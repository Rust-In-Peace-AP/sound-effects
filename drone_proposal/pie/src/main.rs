mod config;
mod drone;
mod controller;
mod packet;
mod network;

use config::parse_config;
use drone::MyDrone;
use controller::SimulationController;
use crossbeam_channel::unbounded;
use std::collections::HashMap;

fn main() {
    let config = parse_config("config.toml");
    let mut controller_drones = HashMap::new();
    let (event_send, event_recv) = unbounded();

    for drone_config in config.drones {
        let (cmd_send, cmd_recv) = unbounded();
        let (pkt_send, pkt_recv) = unbounded();
        controller_drones.insert(drone_config.id, cmd_send.clone());

        let event_send_clone = event_send.clone();
        std::thread::spawn(move || {
            let mut drone = MyDrone::new(
                drone_config.id,
                event_send_clone,
                cmd_recv,
                pkt_recv,
                HashMap::new(),
                drone_config.pdr,
            );
            drone.run();
        });
    }

    let mut controller = SimulationController::new(controller_drones, event_recv);
    controller.run();
}
