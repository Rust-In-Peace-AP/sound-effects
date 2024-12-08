mod tests;
mod sounds;

use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::{fs, thread};
use std::time::Duration;
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{NackType, Nack, Packet, PacketType, FloodResponse, Ack, FloodRequest, Fragment};
use wg_2024::packet::NodeType;
use crate::tests::*;
use crate::sounds::*;

fn contains_pair<K, V>(map: &HashMap<K, V>, key: &K, value: &V) -> bool
where
    K: Eq + std::hash::Hash,
    V: PartialEq,
{
    match map.get(key) {
        Some(v) => v == value,
        None => false,
    }
}

/// Example of drone implementation
struct MyDrone {
    id: NodeId,
    controller_send: Sender<DroneEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    pdr: f32,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashMap<u64, NodeId>,
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
            flood_ids: HashMap::new(),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                // Receiving packets and delegating to the handle_packet function
                recv(self.packet_recv) -> packet => {
                    if let Ok(packet) = packet {
                        self.handle_packet(packet);
                    }
                }
                // Receiving commands
                recv(self.controller_recv) -> command => {
                    if let Ok(command) = command {
                        match command.clone() {

                            DroneCommand::Crash => {

                                self.enter_crashing_behavior();

                                play_sound_from_url(SOUND_CRASH).unwrap();

                                break;
                            }
                            _ => { self.handle_command(command); }
                        }
                    }
                }
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

        Packet::new_nack(rev_routing_header, session_id, Nack {
            fragment_index: 0,
            nack_type,
        })

    }

    fn send_packet(&self, packet: Packet, hop: &NodeId) {
        if let Some(sender) = self.packet_send.get(hop) {
            if let Err(e) = sender.send(packet.clone()) {
                eprintln!("Failed to send packet: {:?}", e);
            }
        }

        self.controller_send.send(DroneEvent::PacketSent(packet.clone())).unwrap();
    }

    fn handle_packet(&mut self, packet: Packet) {

        play_sound_from_url(SOUND_RECEIVED).unwrap();

        let is_flood_request = match packet.pack_type {
            PacketType::FloodRequest(_) => true,
            _ => false,
        };

        let send_sc = match packet.pack_type {
            PacketType::Ack(_) | PacketType::Nack(_) | PacketType::FloodResponse(_) => true,
            _ => false,
        };

        let start_hop_index = packet.routing_header.hop_index;

        // Checking if the packet is for this drone
        if packet.routing_header.hops[start_hop_index] != self.id && !is_flood_request{

            // Creating the nack packet with reversed routing header, hop index incremented because it hasn't been incremented yet and wouldn't split the actual node
            let nack_packet = self.create_nack_packet(&packet.routing_header.hops, start_hop_index+1, NackType::UnexpectedRecipient(self.id), packet.session_id);

            if let Some(next_nack_hop) = nack_packet.routing_header.hops.get(1) {
                // Getting the channel to send the packet to the previous node
                self.send_packet(nack_packet.clone(), &next_nack_hop);
            } else {
                println!("Drone {} cannot send Nack due to missing previous hop.", self.id);
            }

            // Sending the packet to the controller if it's an Ack or Nack or FloodResponse
            if(send_sc) {
                self.controller_send.send(DroneEvent::ControllerShortcut(packet)).unwrap();
            }
            return;
        }

        // Incrementing the hop index
        let new_hop_index = packet.routing_header.hop_index + 1;

        // Checking if the packet has reached the destination
        if new_hop_index == packet.routing_header.hops.len() && !is_flood_request {

            // Creating the nack packet with reversed routing header
            let nack_packet = self.create_nack_packet(&packet.routing_header.hops, new_hop_index.clone(), NackType::DestinationIsDrone, packet.session_id);
            let next_nack_hop = nack_packet.routing_header.hops[1];

            // Getting the channel to send the packet to the previous node
            self.send_packet(nack_packet,&next_nack_hop);

            // No reason to send it to the controller because it's the destination
            return;
        }

        // Getting the next node to send the packet to
        let next_hop = packet.routing_header.hops[new_hop_index];

        // Checking if there is a channel to the next hop. So checking if it's a neighbor
        if self.packet_send.get(&next_hop).is_none() && !is_flood_request {

            // Creating the nack packet with reversed routing header
            let nack_packet = self.create_nack_packet(&packet.routing_header.hops, new_hop_index.clone(), NackType::ErrorInRouting(next_hop), packet.session_id);
            let next_nack_hop = nack_packet.routing_header.hops.get(1);

            if let Some(next_nack_hop) = next_nack_hop {
                // Getting the channel to send the packet to the previous node
                self.send_packet(nack_packet.clone(), &next_nack_hop);
            } else {
                println!("Drone {} cannot send Nack due to missing previous hop.", self.id);
            }

            // Sending the packet to the controller if it's an Ack or Nack or FloodResponse
            if(send_sc) {
                self.controller_send.send(DroneEvent::ControllerShortcut(packet)).unwrap();
            }
            return;

        }

        // Handling the packet based on its type
        match &packet.pack_type {

            PacketType::MsgFragment(_fragment) => {

                // Check to simulate packet loss
                let pdr = self.pdr;
                let random = rand::random::<f32>();

                if random > pdr {

                    play_sound_from_url(SOUND_SENT).unwrap();
                    self.send_packet(packet, &next_hop);

                } else {

                    // Creating the nack packet with reversed routing header
                    play_sound_from_url(SOUND_DROPPED).unwrap();
                    let nack_packet = self.create_nack_packet(&packet.routing_header.hops, new_hop_index.clone(), NackType::Dropped, packet.session_id);

                    let next_nack_hop = nack_packet.routing_header.hops.get(1);

                    if let Some(next_nack_hop) = next_nack_hop {

                        // Getting the channel to send the packet to the previous node
                        self.send_packet(nack_packet.clone(), &next_nack_hop);
                    } else {
                        println!("Drone {} cannot send Nack due to missing previous hop.", self.id);
                    }

                    self.controller_send.send(DroneEvent::PacketDropped(packet)).unwrap();
                }
            },


            PacketType::FloodRequest(flood_request) => {

                let mut flood_request = flood_request.clone();

                if contains_pair(&self.flood_ids, &flood_request.flood_id, &flood_request.initiator_id) {

                    // Creates the routing header reversing the path trace
                    let routing_header = reverse_routing_header(
                        &flood_request.path_trace.iter().map(|(node_id, _)| *node_id).clone().collect::<Vec<NodeId>>(),
                        new_hop_index + 1);

                    let next_hop = routing_header.hops[1];

                    // Creates and sends the response packet
                    let response_packet = Packet::new_flood_response(
                        routing_header.clone(),
                        packet.session_id,
                        FloodResponse {
                            flood_id: flood_request.flood_id,
                            path_trace: flood_request.path_trace.clone(),
                        },
                    );

                    self.send_packet(response_packet, &next_hop);

                    return;
                }

                // Adds the flood id to the list of flood ids
                self.flood_ids.insert(flood_request.flood_id, flood_request.initiator_id);

                // Adds himself to the path trace
                flood_request.path_trace.push((self.id, NodeType::Drone));

                // Filtering the drone that sent the packet from the neighbors of the actual drone
                let neighbors = self.packet_send.keys().filter(|&node_id| {
                    !flood_request.path_trace.iter().any(|(id, _)| id == node_id)
                });

                // If there are no neighbors, creates the response packet
                if neighbors.clone().count() == 0 {

                    // Creates a FloodResponse with the routing header created by reversing the path trace

                    let response_packet = Packet::new_flood_response(
                        reverse_routing_header(
                            &flood_request.path_trace.iter().map(|(node_id, _)| *node_id).clone().collect::<Vec<NodeId>>(),
                            new_hop_index + 1,
                        ),
                        packet.session_id,
                        FloodResponse {
                            flood_id: flood_request.flood_id,
                            path_trace: flood_request.path_trace.clone(),
                        },
                    );

                    // Sends the packet to the previous node of the new routing header reversed that is the previous node of the drone
                    self.send_packet(response_packet.clone(), &response_packet.routing_header.hops[1]);

                    return;
                }

                // Sends the packet to all the neighbors
                for (node_id, sender) in self.packet_send.iter() {

                    // If the node is the one that sent the packet, skip it
                    if flood_request.path_trace.iter().any(|(id, _)| id == node_id) {
                        continue;
                    }

                    // Send the FloodRequest to the neighbors
                    let new_packet = Packet {
                        pack_type: PacketType::FloodRequest(flood_request.clone()),
                        routing_header: packet.routing_header.clone(),
                        session_id: packet.session_id,
                    };

                    if let Err(e) = sender.send(new_packet) {
                        eprintln!("Failed to send packet: {:?}", e);
                    }
                }
            },
            // Don't need to do pattern matching NackType cause you just need to forward them
            _ => {
                self.send_packet(packet, &next_hop);
            }
        }
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {

            DroneCommand::RemoveSender(node_id) => {
                println!("Removing sender for node: {}", node_id);
                if let Some(sender) = self.packet_send.remove(&node_id) {
                    drop(sender);
                    // todo!("Remember to implement the behaviour of the receiver when the sender is dropped in the SC")
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

            _ => {return;},
        }
    }

    // Puts the drone in a crashing state where it processes remaining messages.
    fn enter_crashing_behavior(&mut self) {

        println!("Drone {} entering crashing behavior.", self.id);

        // Remove all neighbors first to ensure no new messages arrive.
        println!("Drone {} removing all neighbors.", self.id);
        for (node_id, sender) in self.packet_send.drain() {

            println!("Dropping sender for node: {}", node_id);
            drop(sender); // Drop each sender to close its channel
        }

        // Continuously process remaining messages until the channel is closed and emptied.
        println!("Drone {} processing remaining messages.", self.id);
        loop {
            match self.packet_recv.recv_timeout(Duration::from_secs(5)) {
                Ok(packet) => {
                    self.process_crashing_message(packet); // Handle messages based on type
                },
                Err(_) => {
                    println!("Drone {} has processed all remaining messages and is now fully crashed.", self.id);
                    break;
                }
            }
        }
    }

    // Processes a single message while in the crashing state.
    fn process_crashing_message(&mut self, packet: Packet) {

        let aux_packet = packet.clone();

        match packet.pack_type {
            PacketType::FloodRequest(_) => {

                println!("Drone {} is crashing: Droppin\
                g FloodRequest packet.", self.id);

            }

            PacketType::Ack(_) | PacketType::Nack(_) | PacketType::FloodResponse(_) => {

                // Forward Ack, Nack, and FloodResponse packets to the next hop
                let next_hop = packet.routing_header.next_hop();
                if let Some(next_hop) = next_hop {
                    self.send_packet(packet, &next_hop);
                } else {
                    println!("Drone {} is crashing: Cannot forward packet due to missing next hop.", self.id);
                }
            }

            PacketType::MsgFragment(_fragment) => {

                // Other packet types will send an ErrorInRouting Nack back
                let nack_packet = self.create_nack_packet(
                    &packet.routing_header.hops,
                    packet.routing_header.hop_index + 1,
                    NackType::ErrorInRouting(self.id),
                    packet.session_id,
                );

                if let Some(prev_hop) = nack_packet.routing_header.hops.get(1) {
                    self.send_packet(nack_packet.clone(), &prev_hop);
                } else {
                    println!("Drone {} is crashing: Cannot send Nack due to missing previous hop.", self.id);
                }
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

// fn parse_config(file: &str) -> Config {
//     let file_str = fs::read_to_string(file).unwrap();
//     toml::from_str(&file_str).unwrap()
// }

fn main() {
    // let config = parse_config("./config.toml");
    //
    // let mut controller_drones = HashMap::new();
    // let (node_event_send, node_event_recv) = unbounded();
    //
    // let mut packet_channels = HashMap::new();
    // for drone in config.drone.iter() {
    //     packet_channels.insert(drone.id, unbounded());
    // }
    // for client in config.client.iter() {
    //     packet_channels.insert(client.id, unbounded());
    // }
    // for server in config.server.iter() {
    //     packet_channels.insert(server.id, unbounded());
    // }
    //
    // let mut handles = Vec::new();
    // for drone in config.drone.into_iter() {
    //     // controller
    //     let (controller_drone_send, controller_drone_recv) = unbounded();
    //     controller_drones.insert(drone.id, controller_drone_send);
    //     let node_event_send = node_event_send.clone();
    //     // packet
    //     let packet_recv = packet_channels[&drone.id].1.clone();
    //     let packet_send = drone
    //         .connected_node_ids
    //         .into_iter()
    //         .map(|id| (id, packet_channels[&id].0.clone()))
    //         .collect(); // Hashmap dei vicini con il loro canale di comunicazione
    //
    //     handles.push(thread::spawn(move || {
    //         let mut drone = MyDrone::new(
    //             drone.id,
    //             node_event_send, // Canale di invio di eventi al controller
    //             controller_drone_recv, // Canale di ricezione di comandi dal controller
    //             packet_recv, // Canale di ricezione di pacchetti
    //             packet_send, // Canale di invio di pacchetti
    //             drone.pdr,
    //         );
    //
    //         drone.run();
    //     }));
    // }
    //
    //
    // let mut controller = SimulationController {
    //     drones: controller_drones,// Hashmap di ogni drone con il suo canale di ricezione
    //     node_event_recv, // Canale di ricezione di eventi dai droni
    // };
    // controller.crash_all();
    //
    // while let Some(handle) = handles.pop() {
    //     handle.join().unwrap();
    // }

    // Tests
    // test_drone_crash_behavior();
    // println!("\nTest passed: test_drone_crash_behavior\n");
    test_drone_communication();
    println!("\nTest passed: test_drone_communication\n");

    return;
}
