use std::{
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};

use camellia::{
    socket::af_xdp::XskSocketBuilder,
    umem::{base::UMemBuilder, shared::SharedAccessor},
};

mod common;
pub use common::*;

#[test]
fn test_packet_forward() {
    env_logger::init();

    let veth_pair = stdenv::setup_veth().unwrap();

    let running = Arc::new(AtomicBool::new(true));
    let ready = Arc::new(AtomicBool::new(false));
    let running_clone = running.clone();
    let running_clone_secondary = running.clone();

    let ready_clone = ready.clone();

    ctrlc::set_handler(move || {
        running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
    })
    .unwrap();

    let forward_namespace = veth_pair.0.right.namespace.clone();
    let forward_namespace_clone = forward_namespace.clone();
    let client_namespace = veth_pair.0.left.namespace.clone();
    let server_namespace = veth_pair.1.right.namespace.clone();

    let broadcase_address = veth::MacAddr::new([0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    let mac_address_client = veth_pair.0.left.mac_addr.clone();
    let mac_address_server = veth_pair.1.right.mac_addr.clone();

    let handle = std::thread::spawn(move || {
        core_affinity::set_for_current(core_affinity::CoreId { id: 1 });

        let _guard = forward_namespace_clone.enter().unwrap();

        let umem = Arc::new(Mutex::new(
            UMemBuilder::new().num_chunks(16384).build().unwrap(),
        ));

        let mut left_socket = XskSocketBuilder::<SharedAccessor>::new()
            .ifname("forward-left")
            .queue_index(0)
            .with_umem(umem.clone())
            .enable_cooperate_schedule()
            .build_shared()
            .unwrap();

        let mut right_socket = XskSocketBuilder::<SharedAccessor>::new()
            .ifname("forward-right")
            .queue_index(0)
            .with_umem(umem)
            .enable_cooperate_schedule()
            .build_shared()
            .unwrap();

        ready_clone.store(true, std::sync::atomic::Ordering::SeqCst);

        while running.load(std::sync::atomic::Ordering::SeqCst) {
            let frames = left_socket.recv_bulk(32).unwrap();
            if frames.len() != 0 {
                log::debug!("receive {} frames from left socket", frames.len());
            }

            let frames: Vec<_> = frames
                .into_iter()
                .map(|frame| {
                    let (ether_header, _remaining) =
                        etherparse::Ethernet2Header::from_slice(frame.raw_buffer()).unwrap();
                    
                    log::debug!("receive packet from right socket: {:?}", ether_header);

                    if ether_header.destination == mac_address_server.bytes()
                        || ether_header.destination == broadcase_address.bytes()
                    {
                        Some(frame)
                    } else {
                        None
                    }
                })
                .flatten()
                .collect();

            let remaining = right_socket.send_bulk(frames).unwrap();
            assert_eq!(remaining.len(), 0);

            let frames = right_socket.recv_bulk(32).unwrap();
            if frames.len() != 0 {
                log::debug!("receive {} frames from right socket", frames.len());
            }

            let frames: Vec<_> = frames
                .into_iter()
                .map(|frame| {
                    let (ether_header, _remaining) =
                        etherparse::Ethernet2Header::from_slice(frame.raw_buffer()).unwrap();

                    log::debug!("receive packet from right socket: {:?}", ether_header);

                    if ether_header.destination == mac_address_client.bytes()
                        || ether_header.destination == broadcase_address.bytes()
                    {
                        Some(frame)
                    } else {
                        None
                    }
                })
                .flatten()
                .collect();

            let remaining = left_socket.send_bulk(frames).unwrap();
            assert_eq!(remaining.len(), 0);
        }
    });

    // let watch_handle = std::thread::spawn(move || {
    //     while running_second_clone.load(std::sync::atomic::Ordering::SeqCst) {
    //         let _guard = forward_namespace.enter().unwrap();

    //         let output = Command::new("ethtool")
    //             .args(["-S", "forward-left"])
    //             .output()
    //             .unwrap();
    //         println!("{}", String::from_utf8(output.stdout).unwrap());

    //         let output = Command::new("ethtool")
    //             .args(["-S", "forward-right"])
    //             .output()
    //             .unwrap();
    //         println!("{}", String::from_utf8(output.stdout).unwrap());

    //         std::thread::sleep(Duration::from_secs(5));
    //     }
    // });

    while !ready.load(std::sync::atomic::Ordering::SeqCst) {}

    let server_handle = std::thread::spawn(move || {
        core_affinity::set_for_current(core_affinity::CoreId { id: 2 });
        let _guard = server_namespace.enter().unwrap();

        std::process::Command::new("iperf3")
            .args(["-s", "-1"])
            .output()
            .unwrap();
    });

    let client_handle = std::thread::spawn(move || {
        core_affinity::set_for_current(core_affinity::CoreId { id: 0 });
        std::thread::sleep(Duration::from_secs(1));
        let _guard = client_namespace.enter().unwrap();

        let mut handle = std::process::Command::new("iperf3")
            .args([
                "-c",
                "192.168.12.1",
                "-t",
                "10",
                // "-J",
                "-C",
                "reno",
            ])
            .spawn()
            .unwrap();

        handle.wait().unwrap();
    });
    server_handle.join().unwrap();
    client_handle.join().unwrap();

    running_clone_secondary.store(false, std::sync::atomic::Ordering::SeqCst);
    handle.join().unwrap();

    // watch_handle.join().unwrap();
}
