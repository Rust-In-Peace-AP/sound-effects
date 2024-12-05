use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Packet {
    pub pack_type: PacketType,
    pub routing_header: SourceRoutingHeader,
    pub session_id: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum PacketType {
    MsgFragment(Fragment),
    Ack(Ack),
    Nack(Nack),
    FloodRequest(FloodRequest),
    FloodResponse(FloodResponse),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SourceRoutingHeader {
    pub hop_index: usize,
    pub hops: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Fragment {
    pub fragment_index: u64,
    pub total_n_fragments: u64,
    pub data: [u8; 128],
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Ack {
    pub fragment_index: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Nack {
    pub fragment_index: u64,
    pub nack_type: NackType,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum NackType {
    ErrorInRouting(u8),
    Dropped,
    UnexpectedRecipient(u8),
    DestinationIsDrone,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FloodRequest {
    pub flood_id: u64,
    pub initiator_id: NodeId,
    pub path_trace: Vec<(NodeId, NodeType)>, // Assume NodeType is defined elsewhere
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FloodResponse {
    pub flood_id: u64,
    pub path_trace: Vec<(NodeId, NodeType)>,
}