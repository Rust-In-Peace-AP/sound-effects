

#[cfg(test)]
mod drone_tests {
    use std::thread;
    use crossbeam_channel::unbounded;
    use wg_2024::{config::Client, tests};
    use wg_2024::packet::{FloodRequest, Fragment};
    use crate::*;

    #[test]
    pub fn test_chain_fragment_ack() {
        wg_2024::tests::generic_chain_fragment_ack::<AmongUsDrone>();
    }

    #[test]
    pub fn test_chain_fragment_drop() {
        wg_2024::tests::generic_chain_fragment_drop::<AmongUsDrone>();
    }

    #[test]
    fn test_fragment_drop() {
        wg_2024::tests::generic_fragment_drop::<AmongUsDrone>();
    }

    #[test]
    fn test_fragment_forward() {
        wg_2024::tests::generic_fragment_forward::<AmongUsDrone>();
    }

 }