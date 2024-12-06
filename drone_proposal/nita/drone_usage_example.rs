#![allow(unused)]

use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use rand::random;

/// Struct definitions based on AP-protocol.md
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

/// Modular and extensible drone implementation
pub struct MyDrone {
    id: u8,
    neighbors: HashMap<u8, Sender<Packet>>,
    receiver: Receiver<Packet>,
    pdr: f32,
    flood_ids: HashSet<u64>,
    controller_sender: Sender<DroneEvent>,
    controller_receiver: Receiver<DroneCommand>,
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
        Self {
            id,
            neighbors,
            receiver,
            pdr,
            flood_ids: HashSet::new(),
            controller_sender,
            controller_receiver,
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
                        self.handle_packet(packet);
                    }
                }
            }
        }
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
                } else if let Some(next_hop) = self.neighbors.get(&packet.routing_header.hops[packet.routing_header.hop_index]) {
                    if let Err(e) = next_hop.send(packet) {
                        self.log_event(format!("Error forwarding MsgFragment: {}", e));
                    }
                } else {
                    self.log_event("Next hop not found for MsgFragment".to_string());
                }
            }
            PacketType::FloodRequest(mut flood_request) => {
                if self.flood_ids.contains(&flood_request.flood_id) {
                    return;
                }
                self.flood_ids.insert(flood_request.flood_id);
                flood_request.path_trace.push((self.id, NodeType::Drone));
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
            PacketType::FloodResponse(flood_response) => {
                self.log_event(format!("FloodResponse received: {:?}", flood_response.path_trace));
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
                self.log_event("Drone crashed. Stopping processing.".to_string());
                return;
            }
        }
    }

    fn log_event(&self, message: String) {
        println!({}{}"Drone {}: {}", chrono::Utc::now(), self.id, message);
    }

    fn report_packet_dropped(&self, packet: &Packet) -> Result<(), Box<dyn Error>> {
        self.controller_sender.send(DroneEvent::PacketDropped(packet.clone()))?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum DroneEvent {
    PacketDropped(Packet),
    PacketSent(Packet),
}