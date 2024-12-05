use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
pub struct DroneConfig {
    pub id: u8,
    pub pdr: f32,
    pub connected_node_ids: Vec<u8>,
}

#[derive(Deserialize)]
pub struct Config {
    pub drones: Vec<DroneConfig>,
}

pub fn parse_config(file: &str) -> Config {
    let content = fs::read_to_string(file).expect("Failed to read config file");
    toml::from_str(&content).expect("Failed to parse TOML config")
}
