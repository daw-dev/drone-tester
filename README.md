# Drone Tester

A simple library to simulate different drone networks and test their performance.

## Usage

### Dependencies

Add the following dependency to your `Cargo.toml`:

```toml
[dependencies]
drone_tester = { git = "https://github.com/daw-dev/drone-tester.git" }
```

### Example

```rust
#[test]
fn flooding() {
    let client = TestNode::with_random_id(vec![1], |params| {
        println!("Client running");
        params
            .packet_send
            .get(&1)
            .unwrap()
            .send(Packet {
                session_id: 0,
                routing_header: SourceRoutingHeader {
                    hops: Vec::new(),
                    hop_index: 0,
                },
                pack_type: PacketType::FloodRequest(FloodRequest {
                    flood_id: 0,
                    initiator_id: params.id,
                    path_trace: vec![(params.id, Client)],
                }),
            })
            .ok();

        thread::sleep(Duration::from_millis(100));

        let mut response_received = false;

        for packet in params.packet_recv.try_iter() {
            if let PacketType::FloodResponse(response) = packet.pack_type {
                println!("Client {} received {:?}", params.id, response);
                assert_eq!(response.flood_id, 0);
                response_received = true;
            }
        }

        assert!(response_received);

        params.end_simulation()
    });
    create_test_environment(
        "topologies/examples/double-chain/topology.toml",
        [client],
        PDRPolicy::Zero,
        create_bagel_bomber,
        create_none_client_server,
        create_none_client_server,
    )
}
```