#![allow(unused)]

use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::{fs, thread};
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::NodeId;
use wg_2024::packet::{Packet, PacketType};

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

    fn create_nack_packet(&self, nack_type: &str, next_hop: NodeId, packet: Packet) -> Packet {
        Packet {
            pack_type: PacketType::Nack(nack_type.to_string()),
            source: self.id,
            next_hop,
            ..packet
        }
    }
    fn handle_packet(&mut self, packet: Packet) {
        // Step 1: Check if hops[hop_index] matches the drone's own NodeId
        if packet.hops[packet.hop_index] != self.id {
            println!("Unexpected recipient: {}, sending Nack", self.id);
            if let Some(sender) = self.packet_send.get(&packet.source) {
                let nack_packet = self.create_nack_packet("UnexpectedRecipient", packet.source, packet);
                let _ = sender.send(nack_packet);
            }
            return;
        }

        // Step 2: Increment hop_index by 1
        let mut packet = packet;
        packet.hop_index += 1;

        // Step 3: Determine if the drone is the final destination
        if packet.hop_index >= packet.hops.len() {
            println!("Drone {} is the final destination, sending Nack", self.id);
            if let Some(sender) = self.packet_send.get(&packet.source) {
                let nack_packet = self.create_nack_packet("DestinationIsDrone", packet.source, packet);
                let _ = sender.send(nack_packet);
            }
            return;
        }
        // Step 4: Identify the next hop
        let next_hop = packet.hops[packet.hop_index];
        if !self.packet_send.contains_key(&next_hop) {
            println!("Error in routing: {}, sending Nack", next_hop);
            if let Some(sender) = self.packet_send.get(&packet.source) {
                let nack_packet = self.create_nack_packet(&format!("ErrorInRouting: {}", next_hop), packet.source, packet);
                let _ = sender.send(nack_packet);
            }
            return;
        }

        // Step 5: Proceed based on the packet type
        match packet.pack_type {
            PacketType::FloodRequest | PacketType::FloodResponse => {
                println!("Handling flood-related packet");
                self.handle_flood_packet(packet);
            }
            PacketType::MsgFragment => {
                // Step 5a: Check for Packet Drop
                if rand::random::<f32>() < self.pdr {
                    // Step 5b: Drop the packet
                    println!("Packet dropped, sending Nack");
                    if let Some(sender) = self.packet_send.get(&packet.source) {
                        let nack_packet = Packet {
                            pack_type: PacketType::Nack("Dropped".to_string()),
                            source: self.id,
                            next_hop: packet.source,
                            hops: packet.hops.iter().rev().cloned().collect(), // Reverse the path
                            ..packet
                        };
                        let _ = sender.send(nack_packet);
                    }
                    return;
                } else {
                    // Step 5c: Send the packet to next_hop
                    println!("Forwarding MsgFragment to next hop: {}", next_hop);
                    if let Some(sender) = self.packet_send.get(&next_hop) {
                        let _ = sender.send(packet);
                    }
                }
            }
            PacketType::Ack | PacketType::Nack | PacketType::FloodResponse => {
                println!("Sending packet back to destination via Simulation Controller");
                self.send_back_to_destination(packet);
            }
            _ => {
                println!("Received unknown packet type");
                // Handle other packet types if necessary
            }
        }
    }

    fn handle_flood_packet(&mut self, packet: Packet) {
        // Implement flood-related packet handling according to Network Discovery Protocol
        println!("Handling flood packet according to Network Discovery Protocol");
        // TODO: Add specific logic for flood packet handling
    }

    fn send_back_to_destination(&mut self, packet: Packet) {
        // Implement sending back to destination through the Simulation Controller
        println!("Sending packet back through the Simulation Controller");
        if let Some(sender) = self.packet_send.get(&packet.source) {
            let _ = sender.send(packet);
        }
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
