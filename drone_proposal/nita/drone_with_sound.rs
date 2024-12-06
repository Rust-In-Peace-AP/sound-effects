use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use rand::random;
use rodio::{Decoder, OutputStream, source::Source};
use std::io::Cursor;
use reqwest::blocking::get;

#[derive(Debug)]
pub struct Packet {
    pub pack_type: PacketType,
    pub routing_header: SourceRoutingHeader,
    pub session_id: u64,
}

#[derive(Debug)]
pub enum PacketType {
    Nack(Nack),
    Ack(Ack),
    MsgFragment(Fragment),
    FloodRequest(FloodRequest),
    FloodResponse(FloodResponse),
}

#[derive(Debug)]
pub struct Nack {
    pub fragment_index: u64,
    pub nack_type: NackType,
}

#[derive(Debug)]
pub enum NackType {
    ErrorInRouting(u8),
    DestinationIsDrone,
    Dropped,
    UnexpectedRecipient(u8),
}

#[derive(Debug)]
pub struct Ack {
    pub fragment_index: u64,
}

#[derive(Debug)]
pub struct Fragment {
    pub fragment_index: u64,
    pub total_n_fragments: u64,
    pub length: u8,
    pub data: [u8; 128],
}

#[derive(Debug)]
pub struct FloodRequest {
    pub flood_id: u64,
    pub initiator_id: u8,
    pub path_trace: Vec<(u8, NodeType)>,
}

#[derive(Debug)]
pub struct FloodResponse {
    pub flood_id: u64,
    pub path_trace: Vec<(u8, NodeType)>,
}

#[derive(Debug)]
pub enum DroneCommand {
    AddSender(u8, Sender<Packet>),
    SetPacketDropRate(f32),
    RemoveSender(u8),
    Crash,
}

#[derive(Debug)]
pub enum NodeType {
    Client,
    Drone,
    Server,
}

#[derive(Debug)]
pub struct SourceRoutingHeader {
    pub hop_index: usize,
    pub hops: Vec<u8>,
}

pub struct MyDrone {
    id: u8,
    neighbors: HashMap<u8, Sender<Packet>>,
    receiver: Receiver<Packet>,
    pdr: f32,
    flood_ids: HashSet<u64>,
    controller_sender: Sender<DroneEvent>,
    controller_receiver: Receiver<DroneCommand>,
    output_stream: OutputStream,  // For sound playback
}

impl MyDrone {
    pub fn new(
        id: u8,
        controller_sender: Sender<DroneEvent>,
        controller_receiver: Receiver<DroneCommand>,
        receiver: Receiver<Packet>,
        neighbors: HashMap<u8, Sender<Packet>>,
        pdr: f32,
    ) -> Self {
        let (_output_stream, stream_handle) = OutputStream::try_default().unwrap();
        Self {
            id,
            neighbors,
            receiver,
            pdr,
            flood_ids: HashSet::new(),
            controller_sender,
            controller_receiver,
            output_stream: stream_handle,
        }
    }

    pub fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.controller_receiver) -> command => {
                    if let Ok(command) = command {
                        self.handle_command(command);
                    }
                },
                recv(self.receiver) -> packet => {
                    if let Ok(packet) = packet {
                        if self.verify_packet(&packet) {
                            self.handle_packet(packet);
                            self.play_sound();  // Play sound when a packet is received
                            // This is a test, so I don't know if it works 100%
                        }
                    }
                }
            }
        }
    }

    // Verifies packet conditions before handling
    fn verify_packet(&self, packet: &Packet) -> bool {
        if packet.routing_header.hops.get(packet.routing_header.hop_index) != Some(&self.id) {
            self.send_nack(
                packet,
                NackType::UnexpectedRecipient(self.id),
            );
            return false;
        }

        if packet.routing_header.hop_index + 1 >= packet.routing_header.hops.len() {
            self.send_nack(
                packet,
                NackType::DestinationIsDrone,
            );
            return false;
        }

        true
    }

    fn play_sound(&self) {
        // Download the sound file from GitHub repository
        let sound_url = "https://example.com/soundfile.wav";  // !NB! Replace with the actual URL of the sound file that we don't have for now
        let response = get(sound_url).unwrap();
        let cursor = Cursor::new(response.bytes().unwrap());

        // Decode and play the sound using rodio
        let source = Decoder::new_wav(cursor).unwrap();
        self.output_stream.play_raw(source.convert_samples()).unwrap();
    }

    fn handle_packet(&mut self, packet: Packet) {
        match packet.pack_type {
            PacketType::Nack(nack) => {
                self.log_event(format!("Nack received: {:?}", nack));
            }
            PacketType::Ack(ack) => {
                self.log_event(format!("Ack received for fragment: {}", ack.fragment_index));
            }
            PacketType::MsgFragment(fragment) => {
                if random::<f32>() < self.pdr {
                    self.log_event(format!("Packet dropped: fragment index {}", fragment.fragment_index));
                    self.report_packet_dropped(&packet).unwrap_or_else(|e| self.log_event(format!("Error reporting packet drop: {}", e)));
                } else if let Some(next_hop) = self.neighbors.get(&packet.routing_header.hops[packet.routing_header.hop_index + 1]) {
                    if let Err(e) = next_hop.send(packet) {
                        self.log_event(format!("Error forwarding MsgFragment: {}", e));
                    }
                } else {
                    self.log_event("Next hop not found for MsgFragment".to_string());
                }
            }
            PacketType::FloodRequest(mut flood_request) => {
                if self.flood_ids.contains(&flood_request.flood_id) {
                    // If the flood_id is already present, we send an error (Nack)
                    self.send_nack(
                        &packet,
                        NackType::ErrorInRouting(1),  // Nack with a generic error
                    );
                    self.log_event(format!("FloodRequest con ID {} già ricevuto. Pacchetto ignorato.", flood_request.flood_id));
                    return;
                }

                // We add the flood_id to track that this request was handled
                self.flood_ids.insert(flood_request.flood_id);
                flood_request.path_trace.push((self.id, NodeType::Drone));

                if self.neighbors.is_empty() {
                    // If there are no neighbors, we respond with FloodResponse
                    let response = Packet {
                        pack_type: PacketType::FloodResponse(FloodResponse {
                            flood_id: flood_request.flood_id,
                            path_trace: flood_request.path_trace.clone(),
                        }),
                        routing_header: SourceRoutingHeader {
                            hop_index: 0,
                            hops: flood_request
                                .path_trace
                                .iter()
                                .map(|(id, _)| *id)
                                .rev()
                                .collect(),
                        },
                        session_id: random(),
                    };
                    self.send_packet(response);
                } else {
                    // Otherwise we forward the request to the neighbors
                    for (neighbor_id, sender) in &self.neighbors {
                        if let Err(e) = sender.send(Packet {
                            pack_type: PacketType::FloodRequest(flood_request.clone()),
                            routing_header: SourceRoutingHeader {
                                hop_index: 0,
                                hops: vec![*neighbor_id],
                            },
                            session_id: random(),
                        }) {
                            self.log_event(format!("Error forwarding FloodRequest to {}: {}", neighbor_id, e));
                        }
                    }
                }
            }
            PacketType::FloodResponse(mut flood_response) => {
                if flood_response.path_trace.is_empty() {
                    self.log_event("FloodResponse reached its initiator".to_string());
                } else {
                    let next_hop = flood_response.path_trace.pop().unwrap().0;
                    if let Some(sender) = self.neighbors.get(&next_hop) {
                        if let Err(e) = sender.send(Packet {
                            pack_type: PacketType::FloodResponse(flood_response),
                            routing_header: SourceRoutingHeader {
                                hop_index: 0,
                                hops: vec![next_hop],
                            },
                            session_id: random(),
                        }) {
                            self.log_event(format!("Error forwarding FloodResponse to {}: {}", next_hop, e));
                        }
                    } else {
                        self.log_event(format!("Next hop for FloodResponse not found: {}", next_hop));
                    }
                }
            }
        }
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::AddSender(node_id, sender) => {
                self.neighbors.insert(node_id, sender);
                self.log_event(format!("Sender added for node {}", node_id));
            }
            DroneCommand::SetPacketDropRate(pdr) => {
                self.pdr = pdr;
                self.log_event(format!("Packet Drop Rate updated to {:.2}", pdr));
            }
            DroneCommand::RemoveSender(node_id) => {
                self.neighbors.remove(&node_id);
                self.log_event(format!("Sender removed for node {}", node_id));
            }
            DroneCommand::Crash => {
                self.log_event("Drone crashed. Stopping processing.");
                return;
            }
        }
    }

    fn send_nack(&self, packet: &Packet, nack_type: NackType) {
        let mut reversed_hops = packet.routing_header.hops.clone();
        reversed_hops.reverse();

        let nack_packet = Packet {
            pack_type: PacketType::Nack(Nack {
                fragment_index: if let PacketType::MsgFragment(frag) = &packet.pack_type {
                    frag.fragment_index
                } else {
                    0
                },
                nack_type,
            }),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: reversed_hops,
            },
            session_id: packet.session_id,
        };

        if let Err(e) = self.controller_sender.send(DroneEvent::PacketDropped(nack_packet)) {
            self.log_event(format!("Error sending Nack: {}", e));
        }
    }

    fn send_packet(&self, packet: Packet) {
        if let Some(next_hop) = packet.routing_header.hops.first() {
            if let Some(sender) = self.neighbors.get(next_hop) {
                if let Err(e) = sender.send(packet) {
                    self.log_event(format!("Error sending packet to {}: {}", next_hop, e));
                }
            } else {
                self.log_event(format!("Next hop {} not found among neighbors", next_hop));
            }
        } else {
            self.log_event("No next hop found in routing header".to_string());
        }
    }

    fn log_event(&self, event: String) {
        println!("Drone {}: {}", self.id, event);
    }

    fn report_packet_dropped(&self, packet: &Packet) -> Result<(), Box<dyn Error>> {
        // Placeholder function
        Ok(())
    }
}

#[derive(Debug)]
pub enum DroneEvent {
    PacketDropped(Packet),
    PacketSent(Packet),
}
