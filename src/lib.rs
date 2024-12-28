mod testing_initializer;
#[cfg(test)]
mod tests;

pub use testing_initializer::create_test_environment;
pub use testing_initializer::PDRPolicy;
pub use testing_initializer::TestNodeInstructions;
pub use topology_setup::Runnable;
use std::collections::HashMap;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

pub struct DummyNode;

impl DummyNode {
    pub fn create_client_server(
        _id: NodeId,
        _packet_recv: Receiver<Packet>,
        _packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Box<dyn Runnable> {
        Box::new(DummyNode)
    }
    pub fn create_drone(
        _id: NodeId,
        _command_recv: Receiver<Packet>,
        _packet_recv: Receiver<Packet>,
        _packet_send: HashMap<NodeId, Sender<Packet>>,
        _pdr: f32,
    ) -> Box<dyn Runnable> {
        Box::new(DummyNode)
    }
}

impl Runnable for DummyNode {
    fn run(&mut self) {}
}
