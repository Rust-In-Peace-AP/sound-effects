use wg_network::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Client,
    Drone,
    Server,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "debug", derive(PartialEq))]
pub struct FloodRequest {
    pub flood_id: u64,
    pub initiator_id: NodeId,
    pub path_trace: Vec<(NodeId, NodeType)>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "debug", derive(PartialEq))]
pub struct FloodResponse {
    pub flood_id: u64,
    pub path_trace: Vec<(NodeId, NodeType)>,
}