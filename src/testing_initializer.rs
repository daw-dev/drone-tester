use crossbeam_channel::{Receiver, Sender};
use rand::{thread_rng, Rng};
use topology_setup::{create_topology_from_config, spawn_threads, ClientServerCreator, Runnable};
use std::collections::{HashMap, HashSet};
use std::thread;
use wg_2024::config::Drone;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

trait TestFunction: Send {
    fn call(&mut self, id: &NodeId, packet_recv: &mut Receiver<Packet>, packet_send: &mut HashMap<NodeId, Sender<Packet>>);
}

impl<F> TestFunction for F
where
    F: FnMut(&NodeId, &mut Receiver<Packet>, &mut HashMap<NodeId, Sender<Packet>>) + Send,
{
    fn call(&mut self, id: &NodeId, packet_recv: &mut Receiver<Packet>, packet_send: &mut HashMap<NodeId, Sender<Packet>>) {
        self(id, packet_recv, packet_send);
    }
}

trait DroneCreatorWithCommandReceiver {
    fn create_drone(&mut self, id: NodeId, command_recv: Receiver<DroneCommand>, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>, pdr: f32) -> Box<dyn Runnable>;
}

impl<F> DroneCreatorWithCommandReceiver for F
where
    F: FnMut(NodeId, Receiver<DroneCommand>, Receiver<Packet>, HashMap<NodeId, Sender<Packet>>, f32) -> Box<dyn Runnable>,
{
    fn create_drone(&mut self, id: NodeId, command_recv: Receiver<DroneCommand>, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>, pdr: f32) -> Box<dyn Runnable> {
        self(id, command_recv, packet_recv, packet_send, pdr)
    }
}

pub struct TestNodeInstructions {
    id: NodeId,
    connected_drone_ids: Vec<NodeId>,
    node_behaviour: Box<dyn TestFunction>,
}

impl TestNodeInstructions {
    pub fn with_node_id(
        id: NodeId,
        connected_drone_ids: Vec<NodeId>,
        node_behaviour: impl TestFunction + 'static,
    ) -> Self {
        TestNodeInstructions {
            id,
            connected_drone_ids,
            node_behaviour: Box::new(node_behaviour),
        }
    }

    pub fn with_random_id(
        connected_drone_ids: Vec<NodeId>,
        node_behaviour: impl TestFunction + 'static,
    ) -> Self {
        TestNodeInstructions {
            id: rand::random(),
            connected_drone_ids,
            node_behaviour: Box::new(node_behaviour),
        }
    }
}

struct TestNode {
    id: NodeId,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    node_behaviour: Option<Box<dyn TestFunction + 'static>>,
}

impl Runnable for TestNode {
    fn run(&mut self) {
        if let Some(mut behaviour) = self.node_behaviour.take() {
            behaviour.call(&self.id, &mut self.packet_recv, &mut self.packet_send);
        }
    }
}

pub enum PDRPolicy {
    Zero,
    Gentle,
    Medium,
    Severe,
    Constant(f32),
    Uniform(f32, f32),
    Unchanged,
}

impl PDRPolicy {
    fn get_pdr(&self, original: f32) -> f32 {
        match self {
            PDRPolicy::Zero => 0.0,
            PDRPolicy::Gentle => thread_rng().gen_range(0.0..0.1),
            PDRPolicy::Medium => thread_rng().gen_range(0.1..0.5),
            PDRPolicy::Severe => thread_rng().gen_range(0.5..0.75),
            PDRPolicy::Constant(pdr) => *pdr,
            PDRPolicy::Uniform(min, max) => thread_rng().gen_range(*min..*max),
            PDRPolicy::Unchanged => original,
        }
    }
}

pub fn create_test_environment(
    topology_file_path: &str,
    mut test_nodes: Vec<TestNodeInstructions>,
    pdr_policy: PDRPolicy,
    mut drone_creator: impl DroneCreatorWithCommandReceiver,
    client_creator: impl ClientServerCreator,
    server_creator: impl ClientServerCreator,
) {
    let mut config = topology_setup::parse_topology_file(topology_file_path);
    let mut test_nodes = test_nodes
        .drain(..)
        .map(|node| (node.id, node))
        .collect::<HashMap<_, _>>();
    let test_nodes_ids = test_nodes.keys().cloned().collect::<HashSet<_>>();

    for test_node in test_nodes.values_mut() {
        let drone_ids = config.drone.iter().map(|drone| drone.id);
        let client_ids = config.client.iter().map(|client| client.id);
        let server_ids = config.server.iter().map(|server| server.id);
        let mut ids = drone_ids.chain(client_ids).chain(server_ids);
        while ids.any(|id| id == test_node.id) {
            test_node.id = rand::random();
        }
        config.drone.push(Drone {
            id: test_node.id,
            pdr: pdr_policy.get_pdr(0.0),
            connected_node_ids: test_node.connected_drone_ids.clone(),
        });
        let connected_ids = test_node.connected_drone_ids.clone();
        for drone in config.drone.iter_mut() {
            drone.pdr = pdr_policy.get_pdr(drone.pdr);
            if connected_ids.contains(&drone.id) {
                drone.connected_node_ids.push(test_node.id);
            }
        }
        for client in config.client.iter_mut() {
            if connected_ids.contains(&client.id) {
                client.connected_drone_ids.push(test_node.id);
            }
        }
        for server in config.server.iter_mut() {
            if connected_ids.contains(&server.id) {
                server.connected_drone_ids.push(test_node.id);
            }
        }
    }

    let mut command_senders = HashMap::new();

    let runnables: HashMap<NodeId, _> = create_topology_from_config(&config, |id, packet_recv, packet_send, pdr| {
        if let Some(test_node) = test_nodes.remove(&id) {
            Box::new(TestNode {
                id,
                packet_recv,
                packet_send,
                node_behaviour: Some(test_node.node_behaviour),
            })
        } else {
            let (command_send, command_recv) = crossbeam_channel::unbounded();
            command_senders.insert(id, command_send);
            drone_creator.create_drone(id, command_recv, packet_recv, packet_send, pdr)
        }
    }, client_creator, server_creator);

    let mut join_handles = spawn_threads(runnables);

    for id in test_nodes_ids.into_iter() {
        if let Some(handle) = join_handles.remove(&id) {
            handle.join().ok();
        }
    }

    for sender in command_senders.values() {
        for id in command_senders.keys() {
            sender.send(DroneCommand::RemoveSender(*id)).ok();
        }
        sender.send(DroneCommand::Crash).ok();
    }

    for handle in join_handles.into_values() {
        handle.join().ok();
    }

    println!("Test ended");
}
