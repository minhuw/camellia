use std::{
    cmp::max,
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use camellia::{
    socket::af_xdp::XskSocketBuilder,
    umem::{
        base::{DedicatedAccessor, UMemBuilder},
        frame::AppFrame,
    },
};
use etherparse::{IpNumber, PacketBuilder};
use std::thread::sleep;
use test_utils::veth::{VethDeviceBuilder, VethPair};

fn setup_veth() -> VethPair {
    let left_device = VethDeviceBuilder::new("test-left")
        .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2a].into())
        .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 11, 1)), 24);

    let right_device = VethDeviceBuilder::new("test-right")
        .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2b].into())
        .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 11, 1)), 24);

    right_device.build(left_device).unwrap()
}

fn build_a_packet(
    veth_pair: &VethPair,
    mut frame: AppFrame<DedicatedAccessor>,
) -> camellia::umem::frame::AppFrame<DedicatedAccessor> {
    let builder = PacketBuilder::ethernet2(
        veth_pair.left.mac_addr.bytes(),
        veth_pair.right.mac_addr.bytes(),
    )
    .ipv4([0, 0, 0, 0], [0, 0, 0, 0], 255);

    let payload = "hello, world!".as_bytes();
    let packet_size = builder.size(payload.len());

    {
        let mut buffer = frame.raw_buffer_append(max(packet_size, 64)).unwrap();
        builder.write(&mut buffer, IpNumber::TCP, payload).unwrap();
    }

    frame
}

#[test]
fn test_packet_io() {
    env_logger::init();

    let veth_pair = setup_veth();

    let umem_left = UMemBuilder::new().num_chunks(4096).build().unwrap();
    let umem_right = UMemBuilder::new().num_chunks(4096).build().unwrap();

    log::info!("Creating socket");

    let mut left_socket = XskSocketBuilder::new()
        .ifname("test-left")
        .queue_index(0)
        .with_umem(umem_left)
        .enable_cooperate_schedule()
        .build()
        .unwrap();

    let mut right_socket = XskSocketBuilder::new()
        .ifname("test-right")
        .queue_index(0)
        .with_umem(umem_right)
        .enable_cooperate_schedule()
        .build()
        .unwrap();

    let mut frame = left_socket.allocate(1).unwrap().pop().unwrap();

    frame = build_a_packet(&veth_pair, frame);

    let packet_size = frame.len();
    assert!(left_socket.send(frame).unwrap().is_none());

    sleep(Duration::from_millis(100));

    let bounced_frame = right_socket.recv().unwrap().unwrap();
    assert_eq!(
        bounced_frame.raw_buffer().len(),
        max(packet_size, bounced_frame.len())
    );

    let mut frame = left_socket.allocate(1).unwrap().pop().unwrap();
    frame = build_a_packet(&veth_pair, frame);
    let packet_size = frame.len();
    assert!(left_socket.send(frame).unwrap().is_none());
    sleep(Duration::from_millis(100));

    let bounced_frame = right_socket.recv().unwrap().unwrap();
    assert_eq!(
        bounced_frame.raw_buffer().len(),
        max(packet_size, bounced_frame.len())
    );
}
