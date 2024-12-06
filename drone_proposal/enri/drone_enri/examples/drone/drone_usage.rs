
use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::{fs, thread};
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{NackType, Nack, Packet, PacketType, FloodResponse, Ack};
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
                +
                recv(self.packet_recv) -> packet => {
                    if let Ok(packet) = packet {
                        self.handle_packet(packet);
                    }
                },
            }
        }
    }
}


impl MyDrone {
    fn reverse_routing_header(hops: &Vec<NodeId>, new_hop_index: usize) -> SourceRoutingHeader {
        let (reverse_hops, _) = hops.split_at(new_hop_index);
        SourceRoutingHeader {
            hops: reverse_hops.iter().rev().cloned().collect(),
            hop_index: 1,
        }
    }

    fn create_nack_packet(
        &self,
        hops: &Vec<NodeId>,
        hop_index: usize,
        nack_type: NackType,
        session_id: u64,
    ) -> Packet {
        let rev_routing_header = Self::reverse_routing_header(hops, hop_index);

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
        let start_hop_index = packet.routing_header.hop_index;

        // Step 1: Verify if packet is for this drone
        if packet.routing_header.hops[start_hop_index] != self.id {
            let nack_packet = self.create_nack_packet(
                &packet.routing_header.hops,
                start_hop_index + 1,
                NackType::UnexpectedRecipient(self.id),
                packet.session_id,
            );
            self.send_packet(nack_packet, &packet.routing_header.hops[start_hop_index - 1]);
            return;
        }

        // Increment hop index
        let new_hop_index = packet.routing_header.hop_index + 1;

        // Step 2: Check if the drone is the final destination
        if new_hop_index == packet.routing_header.hops.len() {
            let nack_packet = self.create_nack_packet(
                &packet.routing_header.hops,
                new_hop_index,
                NackType::DestinationIsDrone,
                packet.session_id,
            );
            self.send_packet(nack_packet, &packet.routing_header.hops[new_hop_index - 2]);
            return;
        }

        // Step 3: Identify the next hop and verify its existence
        let next_hop = packet.routing_header.hops[new_hop_index];
        if self.packet_send.get(&next_hop).is_none() {
            let nack_packet = self.create_nack_packet(
                &packet.routing_header.hops,
                new_hop_index,
                NackType::ErrorInRouting(next_hop),
                packet.session_id,
            );
            self.send_packet(nack_packet, &packet.routing_header.hops[new_hop_index - 2]);
            return;
        }

        // Step 4: Handle packet types
        match packet.pack_type {
            PacketType::FloodRequest(mut flood_request) => {
                if self.flood_ids.contains(&flood_request.flood_id) {
                    let routing_header = Self::reverse_routing_header(
                        &flood_request.path_trace.iter().map(|(node_id, _)| *node_id).collect(),
                        new_hop_index + 1,
                    );
                    let response_packet = Packet {
                        pack_type: PacketType::FloodResponse(FloodResponse {
                            flood_id: flood_request.flood_id,
                            path_trace: flood_request.path_trace.clone(),
                        }),
                        routing_header,
                        session_id: packet.session_id,
                    };
                    self.send_packet(response_packet, &routing_header.hops[1]);
                } else {
                    self.flood_ids.push(flood_request.flood_id);
                    flood_request.path_trace.push((self.id, Default::default()));
                    for neighbor in self.packet_send.keys() {
                        if !flood_request.path_trace.iter().any(|(id, _)| id == neighbor) {
                            let new_packet = Packet {
                                pack_type: PacketType::FloodRequest(flood_request.clone()),
                                routing_header: packet.routing_header.clone(),
                                session_id: packet.session_id,
                            };
                            if let Some(sender) = self.packet_send.get(neighbor) {
                                let _ = sender.send(new_packet);
                            }
                        }
                    }
                }
            }
            PacketType::MsgFragment(_) => {
                if rand::random::<f32>() > self.pdr {
                    self.send_packet(packet, &next_hop);
                }
            }
            _ => self.send_packet(packet, &next_hop),
        }
    }

    fn send_packet(&self, packet: Packet, hop: &NodeId) {
        if let Some(sender) = self.packet_send.get(hop) {
            let _ = sender.send(packet.clone());
        }
        self.controller_send.send(DroneEvent::PacketSent(packet)).unwrap();
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::RemoveSender(node_id) => {
                println!("Removing sender for node: {}", node_id);
                if let Some(sender) = self.packet_send.remove(&node_id) {
                    drop(sender); // Explicitly close the channel with the neighbor drone
                }
            }
            DroneCommand::AddSender(node_id, sender) => {
                println!("Adding sender for node: {}", node_id);
                self.packet_send.insert(node_id, sender);
            }
            DroneCommand::SetPacketDropRate(pdr) => {
                println!("Setting packet drop rate to: {}", pdr);
                self.pdr = pdr;
            }
            DroneCommand::Crash => {
                println!("Drone {} received crash command", self.id);
                self.enter_crashing_behavior();
            }
        }
    }
    fn enter_crashing_behavior(&mut self) {
        println!("Drone {} is entering crashing behavior", self.id);
        loop {
            match self.packet_recv.recv() {
                Ok(packet) => {
                    match packet.pack_type {
                        PacketType::FloodRequest => {
                            println!("Dropping FloodRequest packet during crash");
                            // FloodRequest packets can be lost during crashing behavior
                        }
                        PacketType::Ack | PacketType::Nack | PacketType::FloodResponse => {
                            println!("Forwarding packet: {:?}", packet);
                            // Forward to the next hop if possible
                            if let Some(sender) = self.packet_send.get(&packet.next_hop) {
                                let _ = sender.send(packet);
                            }
                        }
                        _ => {
                            println!("Sending ErrorInRouting Nack back due to crash");
                            // Send an ErrorInRouting Nack back since the drone has crashed
                            if let Some(sender) = self.packet_send.get(&packet.source) {
                                let nack_packet = Packet {
                                    pack_type: PacketType::Nack("ErrorInRouting".to_string()),
                                    source: self.id,
                                    next_hop: packet.source,
                                    ..packet
                                };
                                let _ = sender.send(nack_packet);
                            }
                        }
                    }
                }
                Err(_) => {
                    // Channel is empty and all senders have been removed, drone can finally crash
                    println!("Drone {} has completed crashing", self.id);
                    break;
                }
            }
        }
    }}

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
            .collect();

        handles.push(thread::spawn(move || {
            let mut drone = MyDrone::new(
                drone.id,
                node_event_send,
                controller_drone_recv,
                packet_recv,
                packet_send,
                drone.pdr,
            );

            drone.run();
        }));
    }
    let mut controller = SimulationController {
        drones: controller_drones,
        node_event_recv,
    };
    controller.crash_all();

    while let Some(handle) = handles.pop() {
        handle.join().unwrap();
    }
}
