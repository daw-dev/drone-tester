use crate::topology_setup::{create_intermediate_topology, create_nodes, read_config_file, spawn_threads, IntermediateNode, Node};
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::thread;
use rand::{thread_rng, Rng};
use wg_2024::config::Drone;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

pub struct TestFunctionParams {
    pub id: NodeId,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId, Sender<Packet>>,
    end_simulation: Box<dyn FnOnce()>,
}

impl TestFunctionParams {
    pub fn end_simulation(self) -> TestEnded {
        (self.end_simulation)();
        TestEnded
    }
}

pub struct TestEnded;

type TestFunction = dyn FnOnce(TestFunctionParams) -> TestEnded + Send + 'static;

pub struct TestNode {
    id: NodeId,
    connected_drone_ids: Vec<NodeId>,
    node_behaviour: Box<TestFunction>,
}

impl TestNode {
    pub fn with_node_id(
        id: NodeId,
        connected_drone_ids: Vec<NodeId>,
        node_behaviour: impl FnOnce(TestFunctionParams) -> TestEnded + Send + 'static,
    ) -> Self {
        TestNode {
            id,
            connected_drone_ids,
            node_behaviour: Box::new(node_behaviour),
        }
    }

    pub fn with_random_id(
        connected_drone_ids: Vec<NodeId>,
        node_behaviour: impl FnOnce(TestFunctionParams) -> TestEnded + Send + 'static,
    ) -> Self {
        TestNode {
            id: rand::random(),
            connected_drone_ids,
            node_behaviour: Box::new(node_behaviour),
        }
    }
}

pub enum PDRPolicy {
    Zero,
    Gentle,
    Medium,
    Severe,
}

impl PDRPolicy {
    fn get_prd(&self) -> f32 {
        match self {
            PDRPolicy::Zero => 0.0,
            PDRPolicy::Gentle => thread_rng().gen_range(0.0..0.1),
            PDRPolicy::Medium => thread_rng().gen_range(0.1..0.5),
            PDRPolicy::Severe => thread_rng().gen_range(0.5..0.75),
        }
    }
}

pub fn create_test_environment(
    topology_file_path: &str,
    mut test_nodes: Vec<TestNode>,
    pdr_policy: PDRPolicy,
    drone_creator: impl FnMut(NodeId, Sender<DroneEvent>, Receiver<DroneCommand>, Receiver<Packet>, HashMap<NodeId, Sender<Packet>>, f32) -> Box<dyn Node>,
) {
    let mut config = read_config_file(topology_file_path);

    for test_node in test_nodes.iter_mut() {
        let drone_ids = config.drone.iter().map(|drone| drone.id);
        let client_ids = config.client.iter().map(|client| client.id);
        let server_ids = config.server.iter().map(|server| server.id);
        let mut ids = drone_ids.chain(client_ids).chain(server_ids);
        while ids.any(|id| id == test_node.id) {
            test_node.id = rand::random();
        }
        config.drone.push(Drone {
            id: test_node.id,
            pdr: pdr_policy.get_prd(),
            connected_node_ids: test_node.connected_drone_ids.clone(),
        });
        let connected_ids = test_node.connected_drone_ids.clone();
        for drone in config.drone.iter_mut() {
            drone.pdr = pdr_policy.get_prd();
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

    let (intermediate_nodes, command_senders) = create_intermediate_topology(config);
    let mut test_intermediate_nodes = HashMap::new();
    let mut non_test_intermediate_nodes = HashMap::new();
    for (id, node) in intermediate_nodes.into_iter() {
        if test_nodes.iter().any(|test_node| test_node.id == id) {
            test_intermediate_nodes.insert(id, node);
        } else {
            non_test_intermediate_nodes.insert(id, node);
        }
    }
    let nodes = create_nodes(non_test_intermediate_nodes, drone_creator);
    let join_handles = spawn_threads(nodes);

    let end_simulation = move || {
        for sender in command_senders.values() {
            for id in command_senders.keys() {
                sender.send(DroneCommand::RemoveSender(*id)).ok();
            }
            sender.send(DroneCommand::Crash).ok();
        }
    };

    for test_node in test_nodes.into_iter() {
        if let IntermediateNode::Drone {
            packet_recv,
            packet_send,
            ..
        } = test_intermediate_nodes.remove(&test_node.id).unwrap()
        {
            let end_simulation = end_simulation.clone();
            let test_function = test_node.node_behaviour;
            thread::spawn(move || {
                test_function(TestFunctionParams {
                    id: test_node.id,
                    packet_recv,
                    packet_send,
                    end_simulation: Box::new(end_simulation),
                });
            });
        }
    }

    for handle in join_handles.into_values() {
        handle.join().ok();
    }
    println!("Test ended");
}
