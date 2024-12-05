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
    fn handle_packet(&mut self, mut packet: Packet) {
        // Step 1: Check if current hop matches the drone's NodeId
        if packet.hops[packet.hop_index] != self.id {
            let nack = Packet::nack_unexpected_recipient(self.id);
            self.send_packet_back(nack);
            return;
        }

        // Step 2: Increment hop_index
        packet.hop_index += 1;

        // Step 3: Check if drone is the final destination
        if packet.hop_index >= packet.hops.len() {
            let nack = Packet::nack_destination_is_drone();
            self.send_packet_back(nack);
            return;
        }

        // Step 4: Determine the next hop
        let next_hop = packet.hops[packet.hop_index];
        if !self.packet_send.contains_key(&next_hop) {
            let nack = Packet::nack_error_in_routing(next_hop);
            self.send_packet_back(nack);
            return;
        }

        // Step 5: Handle packet types
        match packet.pack_type {
            PacketType::FloodRequest(_) | PacketType::FloodResponse(_) => {
                // Handle flood messages (Network Discovery Protocol)
                self.handle_flood_packet(packet);
            }
            PacketType::MsgFragment(_) => {
                // Check for Packet Drop
                if self.should_drop_packet() {
                    let nack = Packet::nack_dropped(self.id, &packet);
                    self.send_packet_back(nack);
                } else {
                    // Forward to next hop
                    if let Some(sender) = self.packet_send.get(&next_hop) {
                        sender.send(packet).unwrap();
                    }
                }
            }
            _ => {
                // Return Ack, Nack, or FloodResponse if any error occurs
                self.send_packet_back(packet);
            }
        }
    }

    fn send_packet_back(&self, packet: Packet) {
        // Example sending logic back through Simulation Controller or alternative path
        self.controller_send
            .send(DroneEvent::Packet(packet))
            .unwrap();
    }

    fn handle_flood_packet(&mut self, packet: Packet) {
        // Handle flood logic here
        todo!();
    }

    fn should_drop_packet(&self) -> bool {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        rng.gen::<f32>() < self.pdr
    }
    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            //TO DO(FORSE): aggiungere la condivisione felle modifiche tramite flood response al resto della rete!!!!!!!
            //per tutti i metodi
            DroneCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
            }
            DroneCommand::SetPacketDropRate(new_pdr) => {
                self.pdr = new_pdr;
            }
            DroneCommand::Crash => unreachable!(),
            DroneCommand::RemoveSender(node_id) => {
                self.packet_send.remove(&node_id);
            }
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
