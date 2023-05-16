use std::{
    os::fd::AsRawFd,
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use camellia::{
    socket::af_xdp::XskSocketBuilder,
    umem::{base::UMemBuilder, shared::SharedAccessor},
};
use common::{netns::NetNs, stdenv::setup_veth, veth::MacAddr};
use criterion::{criterion_group, criterion_main, Criterion};
use nix::sys::epoll::{self, EpollEvent};

mod common;
pub use common::*;

fn prepare_env(busy_poll: bool) -> (Arc<NetNs>, Arc<NetNs>, Arc<AtomicBool>, JoinHandle<()>) {
    let veth_pair = setup_veth().unwrap();

    let running = Arc::new(AtomicBool::new(true));
    let ready = Arc::new(AtomicBool::new(false));
    let running_clone = running.clone();

    let ready_clone = ready.clone();

    let client_namespace = veth_pair.0.left.namespace.clone();
    let server_namespace = veth_pair.1.right.namespace.clone();

    let handle = std::thread::spawn(move || {
        core_affinity::set_for_current(core_affinity::CoreId { id: 2 });

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

        if busy_poll {
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
        } else {
            let mut left_event =
                epoll::EpollEvent::new(epoll::EpollFlags::EPOLLIN, left_socket.as_raw_fd() as u64);
            let mut right_event =
                epoll::EpollEvent::new(epoll::EpollFlags::EPOLLIN, right_socket.as_raw_fd() as u64);

            let epfd = epoll::epoll_create().unwrap();
            epoll::epoll_ctl(
                epfd,
                epoll::EpollOp::EpollCtlAdd,
                left_socket.as_raw_fd(),
                Some(&mut left_event),
            )
            .unwrap();
            epoll::epoll_ctl(
                epfd,
                epoll::EpollOp::EpollCtlAdd,
                right_socket.as_raw_fd(),
                Some(&mut right_event),
            )
            .unwrap();

            let mut events = [EpollEvent::empty(); 100];
            let timeout_ms = 1000;

            while running_clone.load(std::sync::atomic::Ordering::SeqCst) {
                let num_events = epoll::epoll_wait(epfd, &mut events, timeout_ms).unwrap();

                for i in 0..num_events {
                    let fd = events[i].data() as i32;
                    if fd == left_socket.as_raw_fd() {
                        let frames = left_socket.recv_bulk(32).unwrap();
                        if frames.len() != 0 {
                            log::debug!("receive {} frames from left socket", frames.len());
                        }

                        let frames: Vec<_> = frames
                            .into_iter()
                            .map(|frame| {
                                let (ether_header, _remaining) =
                                    etherparse::Ethernet2Header::from_slice(frame.raw_buffer())
                                        .unwrap();

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
                    } else if fd == right_socket.as_raw_fd() {
                        let frames = right_socket.recv_bulk(32).unwrap();
                        if frames.len() != 0 {
                            log::debug!("receive {} frames from right socket", frames.len());
                        }

                        let frames: Vec<_> = frames
                            .into_iter()
                            .map(|frame| {
                                let (ether_header, _remaining) =
                                    etherparse::Ethernet2Header::from_slice(frame.raw_buffer())
                                        .unwrap();

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
                    } else {
                        panic!("unexpected fd: {}", fd);
                    }
                }
            }
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
            core_affinity::set_for_current(core_affinity::CoreId { id: 3 });
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
        core_affinity::set_for_current(core_affinity::CoreId { id: 1 });
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

fn busypoll_benchmark(c: &mut Criterion) {
    let (client_ns, server_ns, stop_signal, handle) = prepare_env(true);

    let mut group = c.benchmark_group("Bandwidth");
    group.sample_size(10).bench_function("busy polling", |b| {
        b.iter(|| run_iperf(&client_ns, &server_ns))
    });

    group.finish();

    stop_signal.store(false, std::sync::atomic::Ordering::SeqCst);
    handle.join().unwrap();
}

fn epoll_benchmark(c: &mut Criterion) {
    let (client_ns, server_ns, stop_signal, handle) = prepare_env(false);

    let mut group = c.benchmark_group("Bandwidth");
    group
        .sample_size(10)
        .bench_function("epoll", |b| b.iter(|| run_iperf(&client_ns, &server_ns)));

    group.finish();

    stop_signal.store(false, std::sync::atomic::Ordering::SeqCst);
    handle.join().unwrap();
}

criterion_group!(benches, busypoll_benchmark, epoll_benchmark);
criterion_main!(benches);
