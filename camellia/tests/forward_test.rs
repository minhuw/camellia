use std::{
    net::{IpAddr, Ipv4Addr},
    os::fd::AsRawFd,
    process::Command,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};

use nix::errno::Errno;
use nix::poll::{poll, PollFd, PollFlags};

use anyhow::Result;
use camellia::{
    socket::af_xdp::XskSocketBuilder,
    umem::{base::UMemBuilder, shared::SharedAccessor},
};
use common::{
    netns::NetNs,
    veth::{set_promiscuous, MacAddr, VethDeviceBuilder, VethPair},
};

mod common;

fn setup_veth() -> Result<(VethPair, VethPair)> {
    let client_netns = NetNs::new("client-ns").unwrap();
    let server_netns = NetNs::new("server-ns").unwrap();
    let forward_netns = NetNs::new("forward-ns").unwrap();

    let client_device = VethDeviceBuilder::new("test-left")
        .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2a].into())
        .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 11, 1)), 24)
        .namespace(client_netns.clone());

    let left_device = VethDeviceBuilder::new("forward-left")
        .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2b].into())
        .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 11, 2)), 24)
        .namespace(forward_netns.clone());

    let right_device = VethDeviceBuilder::new("forward-right")
        .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2c].into())
        .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 12, 2)), 24)
        .namespace(forward_netns);

    let server_device = VethDeviceBuilder::new("test-right")
        .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2d].into())
        .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 12, 1)), 24)
        .namespace(server_netns.clone());

    let left_pair = client_device.build(left_device).unwrap();
    let right_pair = right_device.build(server_device).unwrap();

    {
        let _guard = client_netns.enter().unwrap();
        println!("client-ns namespace:");
        display_interface();

        // Set the default route of left and right namespaces
        let mut client_exec_handle = std::process::Command::new("ip")
            .args(["route", "add", "default", "via", "192.168.11.1"])
            .spawn()
            .unwrap();

        client_exec_handle.wait().unwrap();
    }

    {
        let _guard = server_netns.enter().unwrap();
        println!("server-ns namespace:");
        display_interface();
        // Set the default route of left and right namespaces
        let mut right_exec_handle = std::process::Command::new("ip")
            .args(["route", "add", "default", "via", "192.168.12.1"])
            .spawn()
            .unwrap();

        right_exec_handle.wait().unwrap();
    }

    Ok((left_pair, right_pair))
}

fn display_interface() {
    let mut child = Command::new("ip").args(["addr", "ls"]).spawn().unwrap();
    child.wait().unwrap();
}

macro_rules! loop_while_eintr {
    ($poll_expr: expr) => {
        loop {
            match $poll_expr {
                Ok(nfds) => break nfds,
                Err(Errno::EINTR) => (),
                Err(e) => panic!("{}", e),
            }
        }
    };
}

#[test]
fn test_packet_forward() {
    env_logger::init();

    let veth_pair = setup_veth().unwrap();

    {
        let _left_ns_guard = veth_pair.0.right.namespace.enter().unwrap();
        set_promiscuous(veth_pair.0.right.name.as_str());
        set_promiscuous(veth_pair.1.left.name.as_str());
    }

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

    let broadcase_address = MacAddr::new([0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    let mac_address_client = veth_pair.0.left.mac_addr.clone();
    let mac_address_server = veth_pair.1.right.mac_addr.clone();

    let handle = std::thread::spawn(move || {
        let _guard = forward_namespace_clone.enter().unwrap();

        display_interface();

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
                println!("receive {} frames from left socket", frames.len());
            }

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

            let remaining = right_socket.send_bulk(frames).unwrap();
            assert_eq!(remaining.len(), 0);

            let frames = right_socket.recv_bulk(32).unwrap();
            if frames.len() != 0 {
                println!("receive {} frames to right socket", frames.len());
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

    // while !ready.load(std::sync::atomic::Ordering::SeqCst) {}

    {
        println!("try to ping");
        let _left_ns_guard = veth_pair.0.left.namespace.enter().unwrap();
        let handle = std::process::Command::new("ping")
            .args(["192.168.12.1", "-c", "10"])
            .stdout(std::process::Stdio::piped())
            .output()
            .unwrap();

        println!("{}", String::from_utf8(handle.stdout).unwrap());
    }
    running_clone_secondary.store(false, std::sync::atomic::Ordering::SeqCst);

    handle.join().unwrap();
    // watch_handle.join().unwrap();
}
