use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use wg_2024::controller::{DroneCommand, DroneEvent};

pub struct SimulationController {
    drones: HashMap<u8, Sender<DroneCommand>>,
    node_event_recv: Receiver<DroneEvent>,
}

impl SimulationController {
    pub fn new(
        drones: HashMap<u8, Sender<DroneCommand>>,
        node_event_recv: Receiver<DroneEvent>,
    ) -> Self {
        Self { drones, node_event_recv }
    }

    pub fn run(&mut self) {
        while let Ok(event) = self.node_event_recv.recv() {
            match event {
                DroneEvent::PacketSent(packet) => {
                    println!("Packet sent: {:?}", packet);
                }
                DroneEvent::PacketDropped(packet) => {
                    println!("Packet dropped: {:?}", packet);
                }
                DroneEvent::ControllerShortcut(packet) => {
                    println!("Shortcut packet handled: {:?}", packet);
                }
            }
        }
    }
}
