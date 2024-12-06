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

/// Implementazione personalizzata del drone.
/// Il drone gestisce pacchetti e comandi, oltre a partecipare alla rete come nodo intermedio.
struct MyDrone {
    id: NodeId,  // ID univoco del drone nella rete.
    controller_send: Sender<DroneEvent>,  // Canale per inviare eventi al controller della simulazione.
    controller_recv: Receiver<DroneCommand>,  // Canale per ricevere comandi dal controller.
    packet_recv: Receiver<Packet>,  // Canale per ricevere pacchetti dagli altri nodi.
    pdr: f32,  // Probabilità di drop di un pacchetto (Packet Drop Rate).
    packet_send: HashMap<NodeId, Sender<Packet>>,  // Mappa dei vicini e i loro canali per l'invio dei pacchetti.
    flood_ids: Vec<u64>,  // Traccia i flood ID già visti per evitare duplicati.
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
        // Costruttore per inizializzare un nuovo drone.
        Self {
            id,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr,
            flood_ids: Vec::new(), // Inizializza il vettore per tracciare i FloodRequest visti.
        }
    }

    fn run(&mut self) {
        // Ciclo principale del drone: processa comandi e pacchetti.
        loop {
            select_biased! {
                // Gestione prioritaria dei comandi provenienti dal controller.
                recv(self.controller_recv) -> command => {
                    if let Ok(command) = command {
                        if let DroneCommand::Crash = command {
                            // Se il comando è "Crash", interrompi l'esecuzione del drone.
                            println!("drone {} crashed", self.id);
                            self.notify_neighbors_on_crash();
                            break;
                        }
                        self.handle_command(command);  // Processa altri tipi di comandi.
                    }
                }
                +
                // Gestione secondaria dei pacchetti ricevuti dagli altri nodi.
                recv(self.packet_recv) -> packet => {
                    if let Ok(packet) = packet {
                        self.handle_packet(packet);  // Processa il pacchetto ricevuto.
                    }
                },
            }
        }
    }
}

impl MyDrone {
    /// Genera un header di routing inverso per i pacchetti di risposta.
    fn reverse_routing_header(hops: &Vec<NodeId>, new_hop_index: usize) -> SourceRoutingHeader {
        let (reverse_hops, _) = hops.split_at(new_hop_index);  // Dividi gli hop fino al nuovo indice.
        SourceRoutingHeader {
            hops: reverse_hops.iter().rev().cloned().collect(),  // Inverti l'ordine degli hop.
            hop_index: 1,  // L'indice iniziale è sempre 1 nel routing inverso.
        }
    }

    /// Crea un pacchetto Nack per segnalare errori.
    fn create_nack_packet(
        &self,
        hops: &Vec<NodeId>,
        hop_index: usize,
        nack_type: NackType,
        session_id: u64,
    ) -> Packet {
        let rev_routing_header = Self::reverse_routing_header(hops, hop_index);  // Header di routing inverso.

        Packet {
            pack_type: PacketType::Nack(Nack {
                fragment_index: 0,  // Indice del frammento (0, poiché non applicabile qui).
                nack_type,  // Tipo di errore (es. Destinazione errata).
            }),
            routing_header: rev_routing_header,  // Header per tornare al mittente.
            session_id,  // ID sessione originale.
        }
    }

    /// Notifica ai vicini che il drone sta andando in crash.
    fn notify_neighbors_on_crash(&self) {
        for neighbor in self.packet_send.keys() {
            let nack_packet = self.create_nack_packet(
                &vec![self.id, *neighbor],
                1,
                NackType::ErrorInRouting(self.id),
                0, // ID di sessione fittizio per notifiche di crash.
            );
            if let Some(sender) = self.packet_send.get(neighbor) {
                let _ = sender.send(nack_packet);
            }
        }
    }

    /// Processa un pacchetto ricevuto dal drone.
    fn handle_packet(&mut self, packet: Packet) {
        let send_sc = matches!(packet.pack_type, PacketType::Ack(_) | PacketType::Nack(_) | PacketType::FloodResponse(_));
        let start_hop_index = packet.routing_header.hop_index;  // Indice dell'hop attuale.

        // Verifica se il pacchetto è destinato al drone corrente.
        if packet.routing_header.hops[start_hop_index] != self.id {
            let nack_packet = self.create_nack_packet(
                &packet.routing_header.hops,
                start_hop_index + 1,
                NackType::UnexpectedRecipient(self.id),
                packet.session_id,
            );
            self.send_packet(nack_packet, &packet.routing_header.hops[start_hop_index - 1]);

            // Shortcut al controller se il pacchetto è Ack, Nack o FloodResponse.
            if send_sc {
                self.controller_send.send(DroneEvent::ControllerShortcut(packet)).unwrap();
            }
            return;  // Termina il processo per questo pacchetto.
        }

        // Incrementa l'indice hop.
        let new_hop_index = packet.routing_header.hop_index + 1;

        // Verifica se il drone è la destinazione finale.
        if new_hop_index == packet.routing_header.hops.len() {
            let nack_packet = self.create_nack_packet(
                &packet.routing_header.hops,
                new_hop_index,
                NackType::DestinationIsDrone,
                packet.session_id,
            );
            self.send_packet(nack_packet, &packet.routing_header.hops[new_hop_index - 2]);

            // Shortcut al controller se il pacchetto è Ack, Nack o FloodResponse.
            if send_sc {
                self.controller_send.send(DroneEvent::ControllerShortcut(packet)).unwrap();
            }
            return;
        }

        // Identifica il prossimo hop e verifica se esiste.
        let next_hop = packet.routing_header.hops[new_hop_index];
        if self.packet_send.get(&next_hop).is_none() {
            let nack_packet = self.create_nack_packet(
                &packet.routing_header.hops,
                new_hop_index,
                NackType::ErrorInRouting(next_hop),
                packet.session_id,
            );
            self.send_packet(nack_packet, &packet.routing_header.hops[new_hop_index - 2]);

            // Shortcut al controller se il pacchetto è Ack, Nack o FloodResponse.
            if send_sc {
                self.controller_send.send(DroneEvent::ControllerShortcut(packet)).unwrap();
            }
            return;
        }

        // Gestisci il pacchetto in base al tipo.
        match packet.pack_type {
            PacketType::FloodRequest(mut flood_request) => {
                if self.flood_ids.contains(&flood_request.flood_id) {
                    // Caso in cui il flood ID è già stato visto:
                    // Genera un header di routing inverso utilizzando i nodi nel path_trace.
                    let routing_header = Self::reverse_routing_header(
                        &flood_request.path_trace.iter().map(|(node_id, _)| *node_id).collect(),
                        new_hop_index + 1,
                    );

                    // Crea un pacchetto FloodResponse con il path_trace accumulato.
                    let response_packet = Packet {
                        pack_type: PacketType::FloodResponse(FloodResponse {
                            flood_id: flood_request.flood_id,  // Identificativo unico del flood.
                            path_trace: flood_request.path_trace.clone(),  // Traccia del percorso seguito dal flood.
                        }),
                        routing_header,  // Header di routing inverso.
                        session_id: packet.session_id,  // ID sessione originale.
                    };

                    // Invia il pacchetto di risposta al primo nodo della traccia inversa.
                    self.send_packet(response_packet, &routing_header.hops[1]);
                } else {
                    // Caso in cui il flood ID non è stato ancora visto:
                    // Registra il flood ID per evitare duplicati in futuro.
                    self.flood_ids.push(flood_request.flood_id);

                    // Aggiungi il drone corrente alla traccia del percorso.
                    flood_request.path_trace.push((self.id, Default::default()));

                    // Invia la richiesta ai vicini non ancora visitati.
                    for neighbor in self.packet_send.keys() {
                        if !flood_request.path_trace.iter().any(|(id, _)| id == neighbor) {
                            // Crea un nuovo pacchetto FloodRequest con il flood aggiornato.
                            let new_packet = Packet {
                                pack_type: PacketType::FloodRequest(flood_request.clone()),
                                routing_header: packet.routing_header.clone(),
                                session_id: packet.session_id,
                            };

                            // Invia il pacchetto al vicino selezionato.
                            if let Some(sender) = self.packet_send.get(neighbor) {
                                let _ = sender.send(new_packet);
                            }
                        }
                    }
                }
            }
            PacketType::MsgFragment(_) => {
                // Verifica il PDR per decidere se inoltrare o scartare il pacchetto.
                if rand::random::<f32>() > self.pdr {
                    self.send_packet(packet, &next_hop);
                } else {
                    // Crea un Nack per pacchetto droppato.
                    let nack_packet = self.create_nack_packet(&packet.routing_header.hops, new_hop_index, NackType::Dropped, packet.session_id);
                    self.send_packet(nack_packet, &packet.routing_header.hops[new_hop_index - 2]);
                    self.controller_send.send(DroneEvent::PacketDropped(packet)).unwrap();
                }
            }
            _ => self.send_packet(packet, &next_hop),  // Inoltra altri tipi di pacchetti.
        }
    }

    /// Invia un pacchetto al prossimo hop.
    fn send_packet(&self, packet: Packet, hop: &NodeId) {
        if let Some(sender) = self.packet_send.get(hop) {
            let _ = sender.send(packet.clone());
        }
        self.controller_send.send(DroneEvent::PacketSent(packet)).unwrap();
    }

    /// Gestisce i comandi ricevuti dal controller della simulazione.
    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::RemoveSender(node_id) => {
                println!("Removing sender for node: {}", node_id);
                if let Some(sender) = self.packet_send.remove(&node_id) {
                    drop(sender);  // Chiude esplicitamente il canale.
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

    /// Comportamento durante il crash del drone.
    fn enter_crashing_behavior(&mut self) {
        println!("Drone {} is entering crashing behavior", self.id);
        loop {
            match self.packet_recv.recv() {
                Ok(packet) => {
                    match packet.pack_type {
                        PacketType::FloodRequest => {
                            println!("Dropping FloodRequest packet during crash");
                        }
                        PacketType::Ack | PacketType::Nack | PacketType::FloodResponse => {
                            println!("Forwarding packet: {:?}", packet);
                            if let Some(sender) = self.packet_send.get(&packet.next_hop) {
                                let _ = sender.send(packet);
                            }
                        }
                        _ => {
                            println!("Sending ErrorInRouting Nack back due to crash");
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
                    println!("Drone {} has completed crashing", self.id);
                    break;
                }
            }
        }
    }
}
