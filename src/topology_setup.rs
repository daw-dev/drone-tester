use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::thread::JoinHandle;
use std::{fs, thread};
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

pub trait Node: Send {
    fn run(&mut self);
}

impl<T: Drone + Send> Node for T {
    fn run(&mut self) {
        self.run();
    }
}
pub enum IntermediateNode {
    Drone {
        id: NodeId,
        pdr: f32,
        controller_send: Sender<DroneEvent>,
        controller_recv: Receiver<DroneCommand>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    },
    Client {
        id: NodeId,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    },
    Server {
        id: NodeId,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    },
}

pub fn read_config_file(path: &str) -> Config {
    let config_data = fs::read_to_string(path).expect("Unable to read config file");
    let config: Config = toml::from_str(&config_data).expect("Unable to parse TOML");
    config
}

pub fn create_intermediate_topology(
    config: Config,
) -> (
    HashMap<NodeId, IntermediateNode>,
    HashMap<NodeId, Sender<DroneCommand>>,
) {
    //packet channels are the incoming channels of the node ( go to the node)
    let mut packet_senders: HashMap<NodeId, Sender<Packet>> = HashMap::new();
    let mut packet_receivers: HashMap<NodeId, Receiver<Packet>> = HashMap::new();
    let mut command_senders: HashMap<NodeId, Sender<DroneCommand>> = HashMap::new();
    let mut command_receivers: HashMap<NodeId, Receiver<DroneCommand>> = HashMap::new();

    for el in config.drone.iter() {
        let (snd, rcv) = unbounded();
        packet_receivers.insert(el.id, rcv);
        packet_senders.insert(el.id, snd);
        let (snd, rcv) = unbounded();
        command_receivers.insert(el.id, rcv);
        command_senders.insert(el.id, snd);
    }
    for el in config.client.iter() {
        let (snd, rcv) = unbounded();
        packet_receivers.insert(el.id, rcv);
        packet_senders.insert(el.id, snd);
    }
    for el in config.server.iter() {
        let (snd, rcv) = unbounded();
        packet_receivers.insert(el.id, rcv);
        packet_senders.insert(el.id, snd);
    }

    let mut intermediate_nodes = HashMap::new();

    for drone in config.drone.iter() {
        let mut packet_send = HashMap::new();
        for neighbor_id in drone.connected_node_ids.iter() {
            if let Some(snd) = packet_senders.get(neighbor_id) {
                packet_send.insert(*neighbor_id, snd.clone());
            }
        }
        intermediate_nodes.insert(
            drone.id,
            IntermediateNode::Drone {
                id: drone.id,
                pdr: drone.pdr,
                controller_send: unbounded().0,
                controller_recv: command_receivers.remove(&drone.id).unwrap(),
                packet_recv: packet_receivers.remove(&drone.id).unwrap(),
                packet_send,
            },
        );
    }

    fn insert_all_packet_send(
        connected_drone_ids: &[NodeId],
        packet_senders: &HashMap<NodeId, Sender<Packet>>,
    ) -> HashMap<NodeId, Sender<Packet>> {
        let mut packet_send = HashMap::new();
        for neighbor_id in connected_drone_ids.iter() {
            if let Some(snd) = packet_senders.get(neighbor_id) {
                packet_send.insert(*neighbor_id, snd.clone());
            }
        }
        packet_send
    }

    for server in config.server.iter() {
        let packet_send = insert_all_packet_send(&server.connected_drone_ids, &packet_senders);

        intermediate_nodes.insert(
            server.id,
            IntermediateNode::Server {
                id: server.id,
                packet_recv: packet_receivers.remove(&server.id).unwrap(),
                packet_send,
            },
        );
    }
    for client in config.client.iter() {
        let packet_send = insert_all_packet_send(&client.connected_drone_ids, &packet_senders);

        intermediate_nodes.insert(
            client.id,
            IntermediateNode::Client {
                id: client.id,
                packet_recv: packet_receivers.remove(&client.id).unwrap(),
                packet_send,
            },
        );
    }

    (intermediate_nodes, command_senders)
}

pub fn create_nodes(
    intermediate_nodes: HashMap<NodeId, IntermediateNode>,
    mut drone_creator: impl FnMut(
        NodeId,
        Sender<DroneEvent>,
        Receiver<DroneCommand>,
        Receiver<Packet>,
        HashMap<NodeId, Sender<Packet>>,
        f32,
    ) -> Box<dyn Node>,
    mut client_creator: impl FnMut(
        NodeId,
        Receiver<Packet>,
        HashMap<NodeId, Sender<Packet>>,
    ) -> Option<Box<dyn Node>>,
    mut server_creator: impl FnMut(
        NodeId,
        Receiver<Packet>,
        HashMap<NodeId, Sender<Packet>>,
    ) -> Option<Box<dyn Node>>,
) -> HashMap<NodeId, Box<dyn Node>> {
    let mut nodes: HashMap<NodeId, Box<dyn Node>> = HashMap::new();
    for node in intermediate_nodes.into_values() {
        match node {
            IntermediateNode::Drone {
                id,
                pdr,
                controller_send,
                controller_recv,
                packet_recv,
                packet_send,
            } => {
                let boxed_drone = drone_creator(
                    id,
                    controller_send,
                    controller_recv,
                    packet_recv,
                    packet_send,
                    pdr,
                );
                nodes.insert(id, boxed_drone);
            }
            IntermediateNode::Client {
                id,
                packet_recv,
                packet_send,
            } => {
                let boxed_client = client_creator(id, packet_recv, packet_send);
                if let Some(boxed_client) = boxed_client {
                    nodes.insert(id, boxed_client);
                }
            }
            IntermediateNode::Server {
                id,
                packet_recv,
                packet_send,
            } => {
                let boxed_server = server_creator(id, packet_recv, packet_send);
                if let Some(boxed_server) = boxed_server {
                    nodes.insert(id, boxed_server);
                }
            }
        }
    }
    nodes
}

pub fn spawn_threads(nodes: HashMap<NodeId, Box<dyn Node>>) -> HashMap<NodeId, JoinHandle<()>> {
    let mut handles = HashMap::new();
    for (id, mut node) in nodes.into_iter() {
        handles.insert(id, thread::spawn(move || node.run()));
    }
    handles
}
