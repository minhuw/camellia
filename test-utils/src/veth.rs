use anyhow::{anyhow, Result};
use nix::net::if_::if_nametoindex;
use once_cell::sync::OnceCell;
use std::{
    net::IpAddr,
    process::Command,
    sync::{Arc, Weak},
};

use super::netns::NetNs;

pub struct VethPair {
    pub left: Arc<VethDevice>,
    pub right: Arc<VethDevice>,
}

pub struct VethPairBuilder;

impl VethPairBuilder {
    pub fn build(left: VethDeviceBuilder, right: VethDeviceBuilder) -> Result<VethPair> {
        let handle = Command::new("ip")
            .arg("link")
            .arg("add")
            .arg(&left.name)
            .arg("type")
            .arg("veth")
            .arg("peer")
            .arg("name")
            .arg(&right.name)
            .spawn()
            .unwrap();
        let output = handle.wait_with_output().unwrap();

        match output.status.success() {
            true => {}
            false => {
                return Err(anyhow!(
                    "Failed to create veth pair: {}",
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        }
        bind_namespace(&left.name, left.namespace.as_ref().unwrap().clone()).unwrap();
        bind_namespace(&right.name, right.namespace.as_ref().unwrap().clone()).unwrap();

        let left_index = {
            let _guard = left.namespace.as_ref().unwrap().enter().unwrap();
            set_device_l2_addr(&left.name, left.mac_addr.unwrap()).unwrap();
            set_l3_addr(&left.name, left.ip_addr.unwrap().0, left.ip_addr.unwrap().1).unwrap();
            disable_checksum_offload(&left.name).unwrap();
            set_num_rx_queues(&left.name, 1);
            set_num_tx_queues(&left.name, 1);
            up_device(&left.name).unwrap();

            if_nametoindex(left.name.as_str()).unwrap()
        };

        let right_index = {
            let _guard = right.namespace.as_ref().unwrap().enter().unwrap();
            set_device_l2_addr(&right.name, right.mac_addr.unwrap()).unwrap();
            set_l3_addr(
                &right.name,
                right.ip_addr.unwrap().0,
                right.ip_addr.unwrap().1,
            )
            .unwrap();
            disable_checksum_offload(&right.name).unwrap();
            set_num_rx_queues(&right.name, 1);
            set_num_tx_queues(&right.name, 1);
            up_device(&right.name).unwrap();

            if_nametoindex(right.name.as_str()).unwrap()
        };

        let left_device = Arc::new(VethDevice {
            name: left.name,
            index: left_index,
            mac_addr: left.mac_addr.unwrap(),
            ip_addr: left.ip_addr.unwrap(),
            peer: OnceCell::new(),
            namespace: left.namespace.unwrap(),
        });

        let right_device = Arc::new(VethDevice {
            name: right.name,
            index: right_index,
            mac_addr: right.mac_addr.unwrap(),
            ip_addr: right.ip_addr.unwrap(),
            peer: OnceCell::new(),
            namespace: right.namespace.unwrap(),
        });

        left_device.peer.set(Arc::downgrade(&right_device)).unwrap();
        right_device.peer.set(Arc::downgrade(&left_device)).unwrap();

        Ok(VethPair {
            left: left_device,
            right: right_device,
        })
    }
}

impl Drop for VethPair {
    fn drop(&mut self) {
        {
            let _guard = self.left.namespace.enter().unwrap();

            Command::new("ethtool")
                .arg("-S")
                .arg(&self.left.name)
                .spawn()
                .unwrap()
                .wait()
                .unwrap();
        }
        {
            let _guard = self.right.namespace.enter().unwrap();

            Command::new("ethtool")
                .arg("-S")
                .arg(&self.right.name)
                .spawn()
                .unwrap()
                .wait()
                .unwrap();
        }

        let _guard = self.left.namespace.enter().unwrap();

        let output = Command::new("ip")
            .arg("link")
            .arg("del")
            .arg(&self.left.name)
            .output();

        if let Err(e) = output {
            eprintln!("Failed to delete veth pair: {} (you may need to delete it manually with 'sudo ip link del {}')", e, &self.left.name);
        } else {
            let output = output.unwrap();
            if !output.status.success() {
                eprintln!("Failed to delete veth pair: {} (you may need to delete it manually with 'sudo ip link del {}')", String::from_utf8_lossy(&output.stderr), &self.left.name);
            }
        }
    }
}

pub struct VethDevice {
    pub name: String,
    pub index: u32,
    pub mac_addr: MacAddr,
    pub ip_addr: (IpAddr, u8),
    pub peer: OnceCell<Weak<VethDevice>>,
    pub namespace: std::sync::Arc<NetNs>,
}

pub struct VethDeviceBuilder {
    name: String,
    mac_addr: Option<MacAddr>,
    ip_addr: Option<(IpAddr, u8)>,
    namespace: Option<std::sync::Arc<NetNs>>,
}

impl VethDeviceBuilder {
    pub fn new<S: AsRef<str>>(name: S) -> VethDeviceBuilder {
        VethDeviceBuilder {
            name: name.as_ref().to_string(),
            mac_addr: None,
            ip_addr: None,
            namespace: Some(NetNs::current().unwrap()),
        }
    }

    pub fn mac_addr(mut self, mac_addr: MacAddr) -> Self {
        self.mac_addr = Some(mac_addr);
        self
    }

    pub fn ip_addr(mut self, ip_addr: IpAddr, prefix: u8) -> Self {
        self.ip_addr = Some((ip_addr, prefix));
        self
    }

    pub fn namespace(mut self, namespace: std::sync::Arc<NetNs>) -> Self {
        self.namespace = Some(namespace);
        self
    }

    fn complete(&self) -> bool {
        self.mac_addr.is_some() && self.ip_addr.is_some()
    }

    pub fn build(self, peer: VethDeviceBuilder) -> Result<VethPair> {
        if self.complete() && peer.complete() {
            VethPairBuilder::build(self, peer)
        } else {
            Err(anyhow!("VethDeviceBuilder is not complete"))
        }
    }
}

pub fn _down_device(name: &str) -> Result<()> {
    let output = Command::new("ip")
        .arg("link")
        .arg("set")
        .arg("dev")
        .arg(name)
        .arg("down")
        .output()
        .unwrap();

    match output.status.success() {
        true => Ok(()),
        false => Err(anyhow!(String::from_utf8(output.stderr).unwrap())),
    }
}

pub fn up_device(name: &str) -> Result<()> {
    let output = Command::new("ip")
        .arg("link")
        .arg("set")
        .arg("dev")
        .arg(name)
        .arg("up")
        .output()?;

    match output.status.success() {
        true => Ok(()),
        false => Err(anyhow!(String::from_utf8(output.stderr).unwrap())),
    }
}

pub fn set_device_l2_addr(name: &str, mac_addr: MacAddr) -> Result<()> {
    let output = Command::new("ip")
        .arg("link")
        .arg("set")
        .arg("dev")
        .arg(name)
        .arg("address")
        .arg(mac_addr.to_string())
        .output()?;

    match output.status.success() {
        true => Ok(()),
        false => Err(anyhow!(String::from_utf8(output.stderr).unwrap())),
    }
}

pub fn set_l3_addr(name: &str, ip_addr: IpAddr, prefix: u8) -> Result<()> {
    let output = Command::new("ip")
        .arg("address")
        .arg("add")
        .arg(format!("{}/{}", ip_addr, prefix))
        .arg("dev")
        .arg(name)
        .output()?;

    match output.status.success() {
        true => Ok(()),
        false => Err(anyhow!(String::from_utf8(output.stderr).unwrap())),
    }
}

pub fn set_num_rx_queues(name: &str, num_rx_queues: usize) {
    let output = Command::new("ethtool")
        .args(["-L", name, "rx", num_rx_queues.to_string().as_str()])
        .output()
        .unwrap();

    if !output.status.success() {
        eprintln!(
            "Failed to set number of RX queues: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub fn set_num_tx_queues(name: &str, num_tx_queues: usize) {
    let output = Command::new("ethtool")
        .args(["-L", name, "tx", num_tx_queues.to_string().as_str()])
        .output()
        .unwrap();

    if !output.status.success() {
        eprintln!(
            "Failed to set number of TX queues: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub fn set_promiscuous(name: &str) {
    let output = Command::new("ip")
        .args(["link", "set", "dev", name, "promisc", "on"])
        .output()
        .unwrap();

    if !output.status.success() {
        eprintln!(
            "Failed to set promisc: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn remount_sys() -> Result<tempdir::TempDir> {
    let temp_dir = tempdir::TempDir::new("ns_sys").unwrap();
    Command::new("mount")
        .args([
            "-t",
            "sysfs",
            "none",
            temp_dir.path().as_os_str().to_str().unwrap(),
        ])
        .spawn()?
        .wait()?;

    Ok(temp_dir)
}

pub fn set_rps_cores(name: &str, cores: &[usize]) {
    let temp_dir = remount_sys().unwrap();
    let sys_path = format!("{}/class/net/{}/queues", temp_dir.path().display(), name);

    println!("set_rps_cores: sys_path={}", sys_path);

    for entry in std::fs::read_dir(sys_path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("rx-")
        {
            let file = path.join("rps_cpus");
            let bitmap = cores.iter().map(|c| 1u64 << c).fold(0, |acc, m| acc | m);

            println!("write {:x} to {}", bitmap, file.display());

            std::fs::write(file, format!("{:x}", bitmap)).unwrap();
        }
    }
}

pub fn set_preferred_busy_polling(name: &str) {
    let tempdir = remount_sys().unwrap();

    std::fs::write(
        format!(
            "{}/class/net/{}/napi_defer_hard_irqs",
            tempdir.path().display(),
            name
        ),
        "2",
    )
    .unwrap();
    std::fs::write(
        format!(
            "{}/class/net/{}/gro_flush_timeout",
            tempdir.path().display(),
            name
        ),
        "200000",
    )
    .unwrap();
}

pub fn disable_checksum_offload(name: &str) -> Result<()> {
    let output = Command::new("ethtool")
        .args(["-K", name, "tx", "off", "rx", "off"])
        .output()?;

    match output.status.success() {
        true => Ok(()),
        false => Err(anyhow!(String::from_utf8(output.stderr).unwrap())),
    }
}

pub fn bind_namespace(name: &str, netns: std::sync::Arc<NetNs>) -> Result<()> {
    if NetNs::current().unwrap() == netns {
        return Ok(());
    }

    let ns_name = netns.as_ref().path().strip_prefix("/var/run/netns/")?;

    let output = Command::new("ip")
        .args([
            "link",
            "set",
            "dev",
            name,
            "netns",
            ns_name.as_os_str().to_str().unwrap(),
        ])
        .output()?;

    match output.status.success() {
        true => Ok(()),
        false => Err(anyhow!(String::from_utf8(output.stderr).unwrap())),
    }
}

impl VethDevice {
    pub fn peer(&self) -> Arc<VethDevice> {
        self.peer.get().unwrap().upgrade().unwrap()
    }
}

/// Contains the individual bytes of the MAC address.
#[derive(Debug, Clone, Copy, PartialEq, Default, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddr {
    bytes: [u8; 6],
}

impl MacAddr {
    /// Creates a new `MacAddr` struct from the given bytes.
    pub fn new(bytes: [u8; 6]) -> MacAddr {
        MacAddr { bytes }
    }

    /// Returns the array of MAC address bytes.
    pub fn bytes(self) -> [u8; 6] {
        self.bytes
    }
}

impl From<[u8; 6]> for MacAddr {
    fn from(v: [u8; 6]) -> Self {
        MacAddr::new(v)
    }
}

impl std::str::FromStr for MacAddr {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut array = [0u8; 6];

        let mut nth = 0;
        for byte in input.split(|c| c == ':' || c == '-') {
            if nth == 6 {
                return Err(anyhow!("Invalid MAC address: {}", input));
            }

            array[nth] =
                u8::from_str_radix(byte, 16).map_err(|_| anyhow!("Invalid radix digit"))?;

            nth += 1;
        }

        if nth != 6 {
            return Err(anyhow!("Invalid MAC address: {}", input));
        }

        Ok(MacAddr::new(array))
    }
}

impl std::convert::TryFrom<&'_ str> for MacAddr {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl std::convert::TryFrom<std::borrow::Cow<'_, str>> for MacAddr {
    type Error = anyhow::Error;

    fn try_from(value: std::borrow::Cow<'_, str>) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl std::fmt::Display for MacAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let _ = write!(
            f,
            "{:<02X}:{:<02X}:{:<02X}:{:<02X}:{:<02X}:{:<02X}",
            self.bytes[0],
            self.bytes[1],
            self.bytes[2],
            self.bytes[3],
            self.bytes[4],
            self.bytes[5]
        );

        Ok(())
    }
}
