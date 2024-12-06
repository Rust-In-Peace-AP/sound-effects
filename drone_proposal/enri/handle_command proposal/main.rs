impl MyDrone {
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
    }
}

impl MyDrone {
    fn handle_packet(&mut self, packet: Packet) {
        match packet.pack_type {
            PacketType::Nack(_nack) => {
                println!("Handling Nack packet");
                // TODO: Implement Nack packet handling logic
            }
            PacketType::Ack(_ack) => {
                println!("Handling Ack packet");
                // TODO: Implement Ack packet handling logic
            }
            PacketType::Data(_data) => {
                println!("Handling Data packet");
                // TODO: Implement Data packet handling logic
            }
            _ => {
                println!("Received unknown packet type");
                // Handle other packet types if necessary
            }
        }
    }
}
