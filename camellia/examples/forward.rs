use std::{
    os::fd::{AsFd, AsRawFd},
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use camellia::{
    socket::af_xdp::XskSocketBuilder,
    umem::{base::UMemBuilder, shared::SharedAccessorRef},
};
use humansize::{make_format, DECIMAL};
use nix::sys::epoll::{self, EpollCreateFlags, EpollEvent};
use test_utils::{netns::NetNs, stdenv::setup_veth, veth::MacAddr};

fn prepare_env(
    epoll: bool,
    busy_polling: bool,
) -> (Arc<NetNs>, Arc<NetNs>, Arc<AtomicBool>, JoinHandle<()>) {
    log::warn!(
        "set up a {} / {} environment",
        if epoll { "epoll" } else { "polling" },
        if busy_polling {
            "preferred busy polling"
        } else {
            "normal busy pulling"
        }
    );
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

        let mut left_socket_builder = XskSocketBuilder::<SharedAccessorRef>::new()
            .ifname("forward-left")
            .queue_index(0)
            .with_umem(umem.clone())
            .enable_cooperate_schedule();

        if busy_polling {
            left_socket_builder = left_socket_builder.enable_busy_polling();
        }
        let mut left_socket = left_socket_builder.build_shared().unwrap();

        let mut right_socket_builder = XskSocketBuilder::<SharedAccessorRef>::new()
            .ifname("forward-right")
            .queue_index(0)
            .with_umem(umem)
            .enable_cooperate_schedule();

        if busy_polling {
            right_socket_builder = right_socket_builder.enable_busy_polling();
        }

        let mut right_socket = right_socket_builder.build_shared().unwrap();
        let mut total_left_to_right = 0;
        let mut total_right_to_left = 0;
        let batch_size = 32;

        ready_clone.store(true, std::sync::atomic::Ordering::SeqCst);

        if !epoll {
            log::info!("start polling thread");
            while running_clone.load(std::sync::atomic::Ordering::SeqCst) {
                let frames = left_socket.recv_bulk(batch_size).unwrap();

                let frames: Vec<_> = frames
                    .into_iter()
                    .map(|frame| {
                        let (ether_header, _remaining) =
                            etherparse::Ethernet2Header::from_slice(frame.raw_buffer()).unwrap();

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

                total_left_to_right += frames.len();

                if !frames.is_empty() {
                    let remaining = right_socket.send_bulk(frames).unwrap();
                    assert_eq!(remaining.len(), 0);
                }

                let frames = right_socket.recv_bulk(batch_size).unwrap();
                if frames.len() != 0 {
                    log::debug!("receive {} frames from right socket", frames.len());
                }

                let frames: Vec<_> = frames
                    .into_iter()
                    .map(|frame| {
                        let (ether_header, _remaining) =
                            etherparse::Ethernet2Header::from_slice(frame.raw_buffer()).unwrap();

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

                total_right_to_left += frames.len();

                if !frames.is_empty() {
                    let remaining = left_socket.send_bulk(frames).unwrap();
                    assert_eq!(remaining.len(), 0);
                }
            }
            println!(
                "forward thread exits normally. left=>right: {}, right=>left: {}",
                total_left_to_right, total_right_to_left
            );
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
                // let num_events = epoll::epoll_wait(epfd, &mut events, timeout_ms).unwrap();
                for i in 0..num_events {
                    let fd = events[i].data() as i32;
                    if fd == left_socket.as_fd().as_raw_fd() {
                        let frames = left_socket.recv_bulk(batch_size).unwrap();

                        let frames: Vec<_> = frames
                            .into_iter()
                            .map(|frame| {
                                let (ether_header, _remaining) =
                                    etherparse::Ethernet2Header::from_slice(frame.raw_buffer())
                                        .unwrap();

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

                        if !frames.is_empty() {
                            right_socket.send_bulk(frames).unwrap();
                        }
                    } else if fd == right_socket.as_fd().as_raw_fd() {
                        let frames = right_socket.recv_bulk(batch_size).unwrap();
                        let frames: Vec<_> = frames
                            .into_iter()
                            .map(|frame| {
                                let (ether_header, _remaining) =
                                    etherparse::Ethernet2Header::from_slice(frame.raw_buffer())
                                        .unwrap();

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

                        if !frames.is_empty() {
                            left_socket.send_bulk(frames).unwrap();
                        }
                    } else {
                        panic!("unexpected fd: {}", fd);
                    }
                }
            }
        }

        let formatter = make_format(DECIMAL);

        println!(
            "left: rx_batch: {}, rx_packets: {}, rx_bytes: {}, rx_wakeup: {}, tx_batch: {}, tx_packets: {}, tx_bytes: {}, tx_wakeup: {}",
            formatter(left_socket.stat.rx_batch), formatter(left_socket.stat.rx_packets), formatter(left_socket.stat.rx_bytes), formatter(left_socket.stat.rx_wakeup), formatter(left_socket.stat.tx_batch), formatter(left_socket.stat.tx_packets), formatter(left_socket.stat.tx_bytes), formatter(left_socket.stat.tx_wakeup)
        );
        println!(
            "left: rx_batch: {}, rx_packets: {}, rx_bytes: {}, rx_wakeup: {}, tx_batch: {}, tx_packets: {}, tx_bytes: {}, tx_wakeup: {}",
            formatter(right_socket.stat.rx_batch), formatter(right_socket.stat.rx_packets), formatter(right_socket.stat.rx_bytes), formatter(right_socket.stat.rx_wakeup), formatter(right_socket.stat.tx_batch), formatter(right_socket.stat.tx_packets), formatter(right_socket.stat.tx_bytes), formatter(right_socket.stat.tx_wakeup)
        );
    });

    while !ready.load(std::sync::atomic::Ordering::SeqCst) {}

    (client_namespace, server_namespace, running, handle)
}

fn run_iperf(client_ns: &Arc<NetNs>, server_ns: &Arc<NetNs>) {
    let client_ns = client_ns.clone();
    let server_ns = server_ns.clone();

    let server_handle = std::thread::spawn(move || {
        core_affinity::set_for_current(core_affinity::CoreId { id: 3 });
        let _guarad = server_ns.enter().unwrap();

        let output = std::process::Command::new("taskset")
            .args(["-c", "3", "iperf3", "-p", "9000", "-s", "-1"])
            .output()
            .unwrap();

        if !output.status.success() {
            panic!("failed to run iperf3 server");
        };
    });

    std::thread::sleep(Duration::from_secs(1));

    let client_handle = std::thread::spawn(move || {
        core_affinity::set_for_current(core_affinity::CoreId { id: 1 });
        let _guard = client_ns.enter().unwrap();

        let mut output = std::process::Command::new("taskset")
            .args([
                "-c",
                "1",
                "iperf3",
                "-c",
                "192.168.12.1",
                "-p",
                "9000",
                "-t",
                "10",
                "-C",
                "bbr",
            ])
            .spawn()
            .unwrap();

        if !output.wait().unwrap().success() {
            panic!("failed to run iperf3 client");
        };
    });

    client_handle.join().unwrap();
    server_handle.join().unwrap();
}

fn main() {
    let (client_ns, server_ns, stop_signal, handle) = prepare_env(false, false);
    run_iperf(&client_ns, &server_ns);
    stop_signal.store(false, std::sync::atomic::Ordering::SeqCst);
    handle.join().unwrap();
}
