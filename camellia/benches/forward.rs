use std::{
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use camellia::{
    socket::af_xdp::XskSocketBuilder,
    umem::{base::UMemBuilder, shared::SharedAccessor},
};
use common::{
    netns::NetNs,
    stdenv::setup_veth,
    veth::{set_promiscuous, MacAddr},
};
use criterion::{criterion_group, criterion_main, Criterion};

mod common;
pub use common::*;

fn prepare_env() -> (Arc<NetNs>, Arc<NetNs>, Arc<AtomicBool>, JoinHandle<()>) {
    env_logger::init();

    let veth_pair = setup_veth().unwrap();

    {
        let _guard = veth_pair.0.right.namespace.enter().unwrap();
        set_promiscuous(veth_pair.0.right.name.as_str());
        set_promiscuous(veth_pair.1.left.name.as_str());
    }

    let running = Arc::new(AtomicBool::new(true));
    let ready = Arc::new(AtomicBool::new(false));
    let running_clone = running.clone();

    let ready_clone = ready.clone();

    let client_namespace = veth_pair.0.left.namespace.clone();
    let server_namespace = veth_pair.1.right.namespace.clone();

    let handle = std::thread::spawn(move || {
        core_affinity::set_for_current(core_affinity::CoreId { id: 1 });

        let broadcase_address = MacAddr::new([0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        let mac_address_client = veth_pair.0.left.mac_addr.clone();
        let mac_address_server = veth_pair.1.right.mac_addr.clone();

        let _guard = veth_pair.0.right.namespace.enter().unwrap();

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

        while running_clone.load(std::sync::atomic::Ordering::SeqCst) {
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

    while !ready.load(std::sync::atomic::Ordering::SeqCst) {}

    (client_namespace, server_namespace, running, handle)
}

fn run_iperf(client_ns: &Arc<NetNs>, server_ns: &Arc<NetNs>) {
    let client_ns = client_ns.clone();
    let server_ns = server_ns.clone();

    let handle = {
        std::thread::spawn(move || {
            core_affinity::set_for_current(core_affinity::CoreId { id: 0 });
            let _guarad = server_ns.enter().unwrap();
            if !std::process::Command::new("iperf3")
                .args(["-s", "-p", "9000", "-1"])
                .output()
                .unwrap()
                .status
                .success()
            {
                panic!("failed to run iperf3 server");
            }
        })
    };

    std::thread::sleep(Duration::from_secs(1));

    {
        core_affinity::set_for_current(core_affinity::CoreId { id: 2 });
        let _guard = client_ns.enter().unwrap();
        if !std::process::Command::new("iperf3")
            .args([
                "-c",
                "192.168.12.1",
                "-p",
                "9000",
                "-n",
                "1024M",
                "-J",
                "-C",
                "reno",
            ])
            .output()
            .unwrap()
            .status
            .success()
        {
            panic!("failed to run iperf3 client");
        }
    }

    handle.join().unwrap();
}

fn criterion_benchmark(c: &mut Criterion) {
    let (client_ns, server_ns, stop_signal, handle) = prepare_env();

    let mut group = c.benchmark_group("Bandwidth");
    group.sample_size(10).bench_function("busy polling", |b| {
        b.iter(|| run_iperf(&client_ns, &server_ns))
    });

    group.finish();

    stop_signal.store(false, std::sync::atomic::Ordering::SeqCst);
    handle.join().unwrap();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
