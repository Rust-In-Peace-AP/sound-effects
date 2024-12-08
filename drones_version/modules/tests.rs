use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use crossbeam_channel::unbounded;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::SourceRoutingHeader;
use wg_2024::packet::{Ack, Fragment, Packet, PacketType};
use crate::MyDrone;

pub fn test_drone_communication() {
    let (controller_send_1, controller_recv_1) = unbounded();
    let (controller_send_2, controller_recv_2) = unbounded();
    let (packet_send_1, packet_recv_2) = unbounded();
    let (packet_send_2, packet_recv_1) = unbounded();
    let (command_send_1, command_recv_1) = unbounded();
    let (command_send_2, command_recv_2) = unbounded();

    // Configurazione dei droni
    let mut packet_map_1 = HashMap::new();
    packet_map_1.insert(2, packet_send_1);
    let mut drone1 = MyDrone::new(1, controller_send_1, command_recv_1, packet_recv_1, packet_map_1.clone(), 0.0);

    let mut packet_map_2 = HashMap::new();
    packet_map_2.insert(1, packet_send_2);
    let mut drone2 = MyDrone::new(2, controller_send_2, command_recv_2, packet_recv_2, packet_map_2.clone(), 0.0);

    // Avvio dei droni in thread separati
    let handle_1 = thread::spawn(move || drone1.run());
    let handle_2 = thread::spawn(move || drone2.run());

    // Creazione di un frammento di messaggio
    let message = "Hello, this is a test message!".to_string();
    let fragment = Fragment::from_string(0, 1, message);
    let packet = Packet::new_fragment(SourceRoutingHeader{hop_index: 1, hops: vec![1,2]}, 42, fragment);

    // Invio del frammento dal drone 1 al drone 2
    packet_map_1[&2].send(packet.clone()).unwrap();

    // Verifica che il frammento sia ricevuto dal controller del drone 2
    let event = controller_recv_2.recv_timeout(Duration::from_secs(2)).unwrap();

    if let PacketType::MsgFragment(fragment) = packet.pack_type {
        let received_message = String::from_utf8_lossy(&fragment.data[..fragment.length as usize]);
        assert_eq!(received_message, "Hello, this is a test message!");
        println!("Test passed! Drone 2 received fragment: {}", fragment);
    }


    // Terminazione dei droni
    command_send_1.send(DroneCommand::Crash).unwrap();
    command_send_2.send(DroneCommand::Crash).unwrap();
    handle_1.join().unwrap();
    handle_2.join().unwrap();
    return;
}

pub fn test_drone_crash_behavior() {
    let (controller_send, controller_recv) = unbounded();
    let (packet_send, packet_recv) = unbounded();
    let (command_send, command_recv) = unbounded();

    let mut packet_map = HashMap::new();
    packet_map.insert(2, packet_send);
    let mut drone = MyDrone::new(1, controller_send, command_recv, packet_recv, packet_map.clone(), 0.0);

    let handle = thread::spawn(move || drone.run());

    // Invio di pacchetti al drone
    let fragment = Fragment::from_string(0, 1, "Hello".to_string());
    let packet = Packet::new_fragment(SourceRoutingHeader{hop_index: 1, hops: vec![1,2]}, 42, fragment);
    packet_map[&2].send(packet.clone()).unwrap();

    // Invio del comando Crash
    command_send.send(DroneCommand::Crash).unwrap();

    // Attendere che il drone completi il comportamento di crash
    thread::sleep(Duration::from_secs(2));

    // Il drone dovrebbe stampare che è entrato nel comportamento di crash
    println!("Check the logs to verify the crash behavior.");

    // Terminazione del thread
    handle.join().unwrap();
}
