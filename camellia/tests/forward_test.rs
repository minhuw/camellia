use std::net::{IpAddr, Ipv4Addr};

use camellia::{socket::af_xdp::XskSocketBuilder, umem::frame::UMemBuilder};
use common::VethDeviceBuilder;

mod common;

fn setup_veth() -> common::VethPair {
    let left_device = VethDeviceBuilder::new("test-left")
        .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2a].into())
        .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 11, 1)), 24);

    let right_device = VethDeviceBuilder::new("test-right")
        .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2b].into())
        .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 11, 1)), 24);

    right_device.build(left_device).unwrap()
}

#[test]
fn test_socket_create() {
    env_logger::init();

    let _veth_pair = setup_veth();

    let umem_left = UMemBuilder::new().num_chunks(2048).build().unwrap();
    let umem_right = UMemBuilder::new().num_chunks(1024).build().unwrap();

    log::info!("Creating socket");

    let mut left_socket = XskSocketBuilder::new()
        .ifname("test-left")
        .queue_index(0)
        .with_dedicated_umem(umem_left)
        .build()
        .unwrap();

    let mut right_socket = XskSocketBuilder::new()
        .ifname("test-right")
        .queue_index(0)
        .with_dedicated_umem(umem_right)
        .build()
        .unwrap();

    assert!(left_socket.allocate(1).unwrap().len() == 1);
    assert!(right_socket.allocate(1).unwrap().len() == 1);
}
