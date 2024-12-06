#![allow(unused)]

use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::fs;
use std::io::BufReader;
use std::{fs::File, path::Path};
use rand::seq::SliceRandom;
use rodio::{Decoder, OutputStream, Sink};
use crate::config::Config;
use crate::controller::{DroneCommand, DroneEvent};
use crate::network::NodeId;
use crate::packet::{Packet, PacketType, Nack, NackType, SourceRoutingHeader};
use crate::drone::Drone;
/// Example of drone implementation
pub struct MyDrone {
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
    fn handle_packet(&mut self, packet: Packet) {
        match packet.pack_type {
            PacketType::Nack(_) => {
                println!("Drone {} received NACK", self.id);
                self.play_audio("fail");
                self.forward_to_next_hop(packet);
            }
            PacketType::Ack(_) => {
                println!("Drone {} received ACK", self.id);
                self.play_audio("success");
                self.forward_to_next_hop(packet);
            }
            PacketType::MsgFragment(_) => {
                println!("Drone {} received MsgFragment", self.id);
                if self.should_drop_packet() {
                    self.play_audio("fail");
                    self.send_nack(packet, NackType::Dropped);
                } else {
                    self.play_audio("success");
                    self.forward_to_next_hop(packet);
                }
            }
            PacketType::FloodRequest(_) => {
                println!("Drone {} received FloodRequest", self.id);
                self.play_audio("success");
                self.process_flood_request(packet);
            }
            PacketType::FloodResponse(_) => {
                println!("Drone {} received FloodResponse", self.id);
                self.play_audio("success");
                self.forward_to_next_hop(packet);
            }
        }
    }

    fn forward_to_next_hop(&self, mut packet: Packet) {
        let mut routing_header = packet.routing_header.clone();
        routing_header.hop_index += 1;
        if let Some(next_hop) = routing_header.hops.get(routing_header.hop_index) {
            if let Some(sender) = self.packet_send.get(next_hop) {
                packet.routing_header = routing_header;
                let _ = sender.send(packet);
            } else {
                self.send_nack(packet, NackType::ErrorInRouting(*next_hop));
            }
        }
    }

    fn process_flood_request(&self, packet: Packet) {
        let current_hop_index = packet.routing_header.hop_index;
        for (&neighbor_id, sender) in &self.packet_send {
            if packet.routing_header.hops.get(current_hop_index - 1) != Some(&neighbor_id) {
                let _ = sender.send(packet.clone());
            }
        }
    }

    fn send_nack(&self, packet: Packet, nack_type: NackType) {
        let reversed_hops: Vec<NodeId> = packet.routing_header.hops.iter().rev().cloned().collect();
        let nack_packet = Packet {
            pack_type: PacketType::Nack(Nack {
                fragment_index: match packet.pack_type {
                    PacketType::MsgFragment(frag) => frag.fragment_index,
                    _ => 0,
                },
                nack_type,
            }),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: reversed_hops,
            },
            session_id: packet.session_id,
        };
        if let Some(sender) = self.packet_send.get(&nack_packet.routing_header.hops[1]) {
            let _ = sender.send(nack_packet);
        }
    }

    fn should_drop_packet(&self) -> bool {
        let random_value: f32 = rand::random();
        random_value < self.pdr
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {
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

    fn play_audio(&self, category: &str) {
        let category_path = format!("categories/{}", category);
        if let Ok(entries) = fs::read_dir(category_path) {
            let files: Vec<_> = entries
                .filter_map(|entry| entry.ok())
                .filter_map(|e| e.path().to_str().map(String::from))
                .collect();
            if let Some(file_path) = files.choose(&mut rand::thread_rng()) {
                if let Ok(file) = File::open(file_path) {
                    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
                    let sink = Sink::try_new(&stream_handle).unwrap();
                    let source = Decoder::new(BufReader::new(file)).unwrap();
                    sink.append(source);
                    sink.detach();
                }
            }
        }
    }
}
