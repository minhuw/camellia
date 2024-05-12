use std::{
    os::fd::{AsFd, AsRawFd},
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};

use camellia::{
    socket::af_xdp::XskSocketBuilder,
    umem::{base::UMemBuilder, shared::SharedAccessor},
};

use nix::sys::epoll::{self, EpollCreateFlags, EpollEvent};
use test_utils::{stdenv, veth::MacAddr};

fn packet_forward(epoll: bool, busy_polling: bool) {
    let veth_pair = stdenv::setup_veth().unwrap();

    let running = Arc::new(AtomicBool::new(true));
    let ready = Arc::new(AtomicBool::new(false));
    let running_clone = running.clone();
    let running_clone_secondary = running.clone();

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
            UMemBuilder::new().num_chunks(16384 * 16).build().unwrap(),
        ));

        let mut left_socket_builder = XskSocketBuilder::<SharedAccessor>::new()
            .ifname("forward-left")
            .queue_index(0)
            .with_umem(umem.clone())
            .enable_cooperate_schedule();

        if busy_polling {
            left_socket_builder = left_socket_builder.enable_busy_polling();
        }

        let mut left_socket = left_socket_builder.build_shared().unwrap();

        let mut right_socket_builder = XskSocketBuilder::<SharedAccessor>::new()
            .ifname("forward-right")
            .queue_index(0)
            .with_umem(umem)
            .enable_cooperate_schedule();

        if busy_polling {
            right_socket_builder = right_socket_builder.enable_busy_polling();
        }

        let mut right_socket = right_socket_builder.build_shared().unwrap();

        ready_clone.store(true, std::sync::atomic::Ordering::SeqCst);

        if !epoll {
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
            let left_event = epoll::EpollEvent::new(
                epoll::EpollFlags::EPOLLIN,
                left_socket.as_fd().as_raw_fd() as u64,
            );
            let right_event = epoll::EpollEvent::new(
                epoll::EpollFlags::EPOLLIN,
                right_socket.as_fd().as_raw_fd() as u64,
            );

            let epoll = epoll::Epoll::new(EpollCreateFlags::empty()).unwrap();
            epoll.add(&left_socket, left_event).unwrap();
            epoll.add(&right_socket, right_event).unwrap();

            let mut events = [EpollEvent::empty(); 100];
            let timeout_ms: u16 = 1000;

            while running_clone.load(std::sync::atomic::Ordering::SeqCst) {
                let num_events = epoll.wait(&mut events, timeout_ms).unwrap();

                for i in 0..num_events {
                    let fd = events[i].data() as i32;
                    if fd == left_socket.as_fd().as_raw_fd() {
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
                    } else if fd == right_socket.as_fd().as_raw_fd() {
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
        core_affinity::set_for_current(core_affinity::CoreId { id: 3 });
        let _guard = server_namespace.enter().unwrap();

        std::process::Command::new("iperf3")
            .args(["-s", "-1"])
            .output()
            .unwrap();
    });

    let client_handle = std::thread::spawn(move || {
        core_affinity::set_for_current(core_affinity::CoreId { id: 1 });
        std::thread::sleep(Duration::from_secs(1));
        let _guard = client_namespace.enter().unwrap();

        let mut handle = std::process::Command::new("iperf3")
            .args(["-c", "192.168.12.1", "-t", "10", "-C", "reno"])
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

#[test]
fn test_packet_forward() {
    packet_forward(true, false);
    packet_forward(false, false);
    packet_forward(false, true);
}
