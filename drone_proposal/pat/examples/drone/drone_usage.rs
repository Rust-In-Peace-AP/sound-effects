#![allow(unused)]


use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::{fs, thread};
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{NackType, Nack, Packet, PacketType, FloodResponse};
use wg_2024::packet::NackType::UnexpectedRecipient;
use wg_internal::packet::NodeType;

/// Example of drone implementation
struct MyDrone {
    id: NodeId,
    controller_send: Sender<DroneEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    pdr: f32,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: Vec<u64>,
}

impl Drone for MyDrone {
    fn new(
        id: NodeId,
        controller_send: Sender<DroneEvent>,
        controller_recv: Receiver<DroneCommand>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
        pdr: f32,

    ) -> Self {
        Self {
            id,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr,
            flood_ids: Vec::new(),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.controller_recv) -> command => {
                    if let Ok(command) = command {
                        if let DroneCommand::Crash = command {
                            println!("drone {} crashed", self.id);
                            break;
                        }
                        self.handle_command(command);
                    }
                }
                recv(self.packet_recv) -> packet => {
                    if let Ok(packet) = packet {
                        self.handle_packet(packet);
                    }
                },
            }
        }
    }
}


// Reversing the routing header
fn reverse_routing_header(hops: &Vec<NodeId>, new_hop_index: usize) -> SourceRoutingHeader {
    let (reverse_hops, _) = hops.split_at(new_hop_index);
    SourceRoutingHeader {
        hops: reverse_hops.iter().rev().cloned().collect(),
        hop_index: 1,
    }
}

impl MyDrone {

    fn create_nack_packet(&self, hops: &Vec<NodeId>, hop_idx: usize, nack_type: NackType, session_id: u64) -> Packet {

        let rev_routing_header = reverse_routing_header(hops, hop_idx);

        Packet {
            pack_type: PacketType::Nack(Nack {
                fragment_index: 0,
                nack_type,
            }),
            routing_header: rev_routing_header,
            session_id,
        }
    }


    fn handle_packet(&mut self, packet: Packet) {

        // Checking if the packet is for this drone
        if packet.routing_header.hops[packet.routing_header.hop_index] != self.id {

            // Creating the nack packet with reversed routing header, hop index incremented because it hasn't been incremented yet and wouldn't split the actual node
            let nack_packet = self.create_nack_packet(&packet.routing_header.hops, packet.routing_header.hop_index+1, UnexpectedRecipient(self.id), packet.session_id);

            // Getting the channel to send the packet to the previous node
            if let Some(sender) = self.packet_send.get(&packet.routing_header.hops[packet.routing_header.hop_index-1]) {
                if let Err(e) = sender.send(nack_packet) {
                    eprintln!("Failed to send NACK packet: {:?}", e);
                }
            }

            return;
        }

        // Incrementing the hop index
        let new_hop_index = packet.routing_header.hop_index + 1;

        // Checking if the packet has reached the destination
        if new_hop_index == packet.routing_header.hops.len() {

            // Creating the nack packet with reversed routing header
            let nack_packet = self.create_nack_packet(&packet.routing_header.hops, new_hop_index.clone(), NackType::DestinationIsDrone, packet.session_id);

            // Getting the channel to send the packet to the previous node
            if let Some(sender) = self.packet_send.get(&packet.routing_header.hops[new_hop_index-2]) {
                if let Err(e) = sender.send(nack_packet) {
                    eprintln!("Failed to send NACK packet: {:?}", e);
                }
            }
            return;
        }

        // Getting the next node to send the packet to
        let next_hop = packet.routing_header.hops[new_hop_index];

        // Checking if there is a channel to the next hop. So checking if it's a neighbor
        if self.packet_send.get(&next_hop).is_none() {

            // Creating the nack packet with reversed routing header
            let nack_packet = self.create_nack_packet(&packet.routing_header.hops, new_hop_index.clone(), NackType::ErrorInRouting(next_hop), packet.session_id);

            // Getting the channel to send the packet to the previous node
            if let Some(sender) = self.packet_send.get(&packet.routing_header.hops[new_hop_index-2]) {
                if let Err(e) = sender.send(nack_packet) {
                    eprintln!("Failed to send NACK packet: {:?}", e);
                }
            }
            return;
        }

        // Handling the packet based on its type
        // In case of Nack, Ack, FloodRequest, FloodResponse the drone will forward
        // the packet through the SC
        match &packet.pack_type {

            // Don't need to do pattern matching on Ack and Nack cause you just need to forward them
            PacketType::Nack(_nack) => {

                if let Some(sender) = self.packet_send.get(&next_hop) {
                    if let Err(e) = sender.send(packet) {
                        eprintln!("Failed to send packet: {:?}", e);
                    }
                }
            },
            PacketType::Ack(_ack) => {

                if let Some(sender) = self.packet_send.get(&next_hop) {
                    if let Err(e) = sender.send(packet) {
                        eprintln!("Failed to send packet: {:?}", e);
                    }
                }
            },

            PacketType::MsgFragment(_fragment) => {

                // Controllo per simulare la perdita di pacchetti
                let pdr = self.pdr;
                let random = rand::random::<f32>();

                if random > pdr {

                    if let Some(sender) = self.packet_send.get(&next_hop) {
                        if let Err(e) = sender.send(packet) {
                            eprintln!("Failed to send packet: {:?}", e);
                        }
                    }

                } else {

                    // Creating the nack packet with reversed routing header
                    let nack_packet = self.create_nack_packet(&packet.routing_header.hops, new_hop_index.clone(), NackType::Dropped, packet.session_id);

                    if let Some(sender) = self.packet_send.get(&packet.routing_header.hops[new_hop_index-1]) {
                        if let Err(e) = sender.send(nack_packet) {
                            eprintln!("Failed to send NACK packet: {:?}", e);
                        }
                    }
                }
            },

            PacketType::FloodRequest(mut flood_request) => {

                if self.flood_ids.contains(&flood_request.flood_id) {
                    // todo!("Implement the floodResponse");
                    return;
                }
                // todo!("Pensa se puoi invertire il path_trace e renderlo il rev_routing_header invece di ruotarlo con la funzione");
                // Adds the flood id to the list of flood ids
                self.flood_ids.push(flood_request.flood_id);

                // Adds himself to the path trace
                flood_request.path_trace.push((self.id, NodeType::Drone));

                // Gets the number of neighbors, if 0 create and send the flood response
                let ngbhs_count = self.packet_send.len();
                if ngbhs_count == 0 {

                    let flood_response = FloodResponse {
                        flood_id: flood_request.flood_id,
                        path_trace: flood_request.path_trace.clone(),
                    };

                    let rev_routing_header = reverse_routing_header(&packet.routing_header.hops, new_hop_index);

                    let new_packet = Packet {
                        pack_type: PacketType::FloodResponse(flood_response),
                        routing_header: rev_routing_header,
                        session_id: packet.session_id,
                    };

                    if let Some(sender) = self.packet_send.get(&new_packet.routing_header.hops[1]) {
                        if let Err(e) = sender.send(new_packet) {
                            eprintln!("Failed to send packet: {:?}", e);
                        }
                    }

                    return;
                }

                for (node_id, sender) in self.packet_send.iter() {

                    if *node_id == packet.routing_header.hops[packet.routing_header.hop_index] {
                        continue;
                    }

                    let mut new_hops = packet.routing_header.hops.clone();
                    new_hops.push(*node_id);

                    let new_packet = Packet {
                        pack_type: PacketType::FloodRequest(flood_request.clone()),
                        routing_header: SourceRoutingHeader {
                            hops: new_hops,
                            hop_index: new_hop_index,
                        },
                        session_id: packet.session_id,
                    };

                    sender.send(new_packet).unwrap();
                }
            },
            PacketType::FloodResponse(_flood_response) => todo!(),
        }
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
            },
            DroneCommand::SetPacketDropRate(_pdr) => todo!(),
            DroneCommand::Crash => unreachable!(),
            DroneCommand::RemoveSender(_node_id) => todo!(),
        }
    }
}

struct SimulationController {
    drones: HashMap<NodeId, Sender<DroneCommand>>,
    node_event_recv: Receiver<DroneEvent>,
}

impl SimulationController {
    fn crash_all(&mut self) {
        for (_, sender) in self.drones.iter() {
            sender.send(DroneCommand::Crash).unwrap();
        }
    }
}

fn parse_config(file: &str) -> Config {
    let file_str = fs::read_to_string(file).unwrap();
    toml::from_str(&file_str).unwrap()
}

fn main() {
    let config = parse_config("./config.toml");

    let mut controller_drones = HashMap::new();
    let (node_event_send, node_event_recv) = unbounded();

    let mut packet_channels = HashMap::new();
    for drone in config.drone.iter() {
        packet_channels.insert(drone.id, unbounded());
    }
    for client in config.client.iter() {
        packet_channels.insert(client.id, unbounded());
    }
    for server in config.server.iter() {
        packet_channels.insert(server.id, unbounded());
    }

    let mut handles = Vec::new();
    for drone in config.drone.into_iter() {
        // controller
        let (controller_drone_send, controller_drone_recv) = unbounded();
        controller_drones.insert(drone.id, controller_drone_send);
        let node_event_send = node_event_send.clone();
        // packet
        let packet_recv = packet_channels[&drone.id].1.clone();
        let packet_send = drone
            .connected_node_ids
            .into_iter()
            .map(|id| (id, packet_channels[&id].0.clone()))
            .collect(); // Hashmap dei vicini con il loro canale di comunicazione

        handles.push(thread::spawn(move || {
            let mut drone = MyDrone::new(
                drone.id,
                node_event_send, // Canale di invio di eventi al controller
                controller_drone_recv, // Canale di ricezione di comandi dal controller
                packet_recv, // Canale di ricezione di pacchetti
                packet_send, // Canale di invio di pacchetti
                drone.pdr,
            );

            drone.run();
        }));
    }
    let mut controller = SimulationController {
        drones: controller_drones,// Hashmap di ogni drone con il suo canale di ricezione
        node_event_recv, // Canale di ricezione di eventi dai droni
    };
    controller.crash_all();

    while let Some(handle) = handles.pop() {
        handle.join().unwrap();
    }
}
