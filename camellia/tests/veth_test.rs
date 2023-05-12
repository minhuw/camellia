use common::veth::VethDeviceBuilder;
use std::net::{IpAddr, Ipv4Addr};
use std::process::Command;

mod common;
pub use common::*;

#[test]
fn test_veth_setup() {
    {
        let left_device = VethDeviceBuilder::new("test-left")
            .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2a].into())
            .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 11, 1)), 24);

        let right_device = VethDeviceBuilder::new("test-right")
            .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2b].into())
            .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 11, 1)), 24);

        let _veth_pair = right_device.build(left_device).unwrap();
        let output = Command::new("ip")
            .args(["link", "ls"])
            .output()
            .expect("fail to run ip link ls");
        let output = String::from_utf8_lossy(&output.stdout);

        assert!(output.contains("test-left@test-right"));
        assert!(output.contains("test-right@test-left"));
    }

    let output = Command::new("ip")
        .args(["link", "ls"])
        .output()
        .expect("fail to run ip link ls");
    let output = String::from_utf8_lossy(&output.stdout);

    assert!(!output.contains("test-left@test-right"));
    assert!(!output.contains("test-right@test-left"));
}
