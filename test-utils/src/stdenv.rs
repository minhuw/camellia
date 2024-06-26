use std::net::{IpAddr, Ipv4Addr};

use anyhow::Result;

use crate::{
    netns::NetNs,
    veth::{set_preferred_busy_polling, set_promiscuous, set_rps_cores},
    veth::{VethDeviceBuilder, VethPair},
};

pub fn setup_veth() -> Result<(VethPair, VethPair)> {
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
        .namespace(forward_netns.clone());

    let server_device = VethDeviceBuilder::new("test-right")
        .mac_addr([0x38, 0x7e, 0x58, 0xe7, 0x87, 0x2d].into())
        .ip_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 12, 1)), 24)
        .namespace(server_netns.clone());

    let left_pair = client_device.build(left_device).unwrap();
    let right_pair = right_device.build(server_device).unwrap();

    {
        let _guard = client_netns.enter().unwrap();

        // Set the default route of left and right namespaces
        std::process::Command::new("ip")
            .args(["route", "add", "default", "via", "192.168.11.1"])
            .spawn()
            .unwrap()
            .wait()
            .unwrap();

        std::process::Command::new("tc")
            .args([
                "qdisc",
                "add",
                "dev",
                left_pair.left.name.as_str(),
                "root",
                "handle",
                "1:",
                "fq",
            ])
            .spawn()
            .unwrap()
            .wait()
            .unwrap();

        set_rps_cores(left_pair.left.name.as_str(), &[1]);
    }

    {
        let _guard = server_netns.enter().unwrap();

        // Set the default route of left and right namespaces
        std::process::Command::new("ip")
            .args(["route", "add", "default", "via", "192.168.12.1"])
            .spawn()
            .unwrap()
            .wait()
            .unwrap();

        std::process::Command::new("tc")
            .args([
                "qdisc",
                "add",
                "dev",
                right_pair.right.name.as_str(),
                "root",
                "handle",
                "1:",
                "fq",
            ])
            .spawn()
            .unwrap()
            .wait()
            .unwrap();

        set_rps_cores(right_pair.right.name.as_str(), &[3]);
    }

    {
        let _guard = forward_netns.enter().unwrap();
        set_promiscuous(left_pair.right.name.as_str());
        set_promiscuous(right_pair.left.name.as_str());
        set_rps_cores(left_pair.right.name.as_str(), &[2]);
        set_rps_cores(right_pair.left.name.as_str(), &[2]);
        set_preferred_busy_polling(left_pair.right.name.as_str());
        set_preferred_busy_polling(right_pair.left.name.as_str());
    }

    Ok((left_pair, right_pair))
}
