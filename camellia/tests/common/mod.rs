use std::{rc::{Rc, Weak}, process::Command, net::IpAddr};
use anyhow::{anyhow, Result};
use nix::net::if_::if_nametoindex;
use once_cell::unsync::OnceCell;

pub struct VethPair {
    pub left: Rc<VethDevice>,
    pub right: Rc<VethDevice>,
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
            .spawn()?;
        let output = handle.wait_with_output()?;

        match output.status.success() {
            true => {}
            false => {
                return Err(anyhow!(
                    "Failed to create veth pair: {}",
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        }

        set_device_l2_addr(&left.name, left.mac_addr.unwrap())?;
        set_device_l2_addr(&right.name, right.mac_addr.unwrap())?;
        set_l3_addr(&left.name, left.ip_addr.unwrap().0, left.ip_addr.unwrap().1)?;
        set_l3_addr(
            &right.name,
            right.ip_addr.unwrap().0,
            right.ip_addr.unwrap().1,
        )?;
        disable_checksum_offload(&left.name)?;
        disable_checksum_offload(&right.name)?;

        up_device(&left.name)?;
        up_device(&right.name)?;

        let left_name = left.name.as_str();
        let right_name = right.name.as_str();

        let left_device = Rc::new(VethDevice {
            name: left_name.to_string(),
            index: if_nametoindex(left_name)?,
            mac_addr: left.mac_addr.unwrap(),
            ip_addr: left.ip_addr.unwrap(),
            peer: OnceCell::new(),
        });

        let right_device = Rc::new(VethDevice {
            name: right_name.to_string(),
            index: if_nametoindex(right_name)?,
            mac_addr: right.mac_addr.unwrap(),
            ip_addr: right.ip_addr.unwrap(),
            peer: OnceCell::new(),
        });

        left_device.peer.set(Rc::downgrade(&right_device)).unwrap();
        right_device.peer.set(Rc::downgrade(&left_device)).unwrap();

        Ok(VethPair {
            left: left_device,
            right: right_device,
        })
    }
}

impl Drop for VethPair {
    fn drop(&mut self) {
        let output = Command::new("ip")
            .arg("link")
            .arg("del")
            .arg(&self.left.peer().name)
            .output();

        if let Err(e) = output {
            eprintln!("Failed to delete veth pair: {} (you may need to delete it manually with 'sudo ip link del {}')", e, &self.left.peer().name);
        } else {
            let output = output.unwrap();
            if !output.status.success() {
                eprintln!("Failed to delete veth pair: {} (you may need to delete it manually with 'sudo ip link del {}')", String::from_utf8_lossy(&output.stderr), &self.left.peer().name);
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
}

pub struct VethDeviceBuilder {
    name: String,
    mac_addr: Option<MacAddr>,
    ip_addr: Option<(IpAddr, u8)>,
}

impl VethDeviceBuilder {
    pub fn new<S: AsRef<str>>(name: S) -> VethDeviceBuilder {
        VethDeviceBuilder {
            name: name.as_ref().to_string(),
            mac_addr: None,
            ip_addr: None,
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

pub fn down_device(name: &str) -> Result<()> {
    let output = Command::new("ip")
        .arg("link")
        .arg("set")
        .arg("dev")
        .arg(name)
        .arg("down")
        .output()?;

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

pub fn disable_checksum_offload(name: &str) -> Result<()> {
    let output = Command::new("ethtool")
        .args(["-K", name, "tx", "off", "rx", "off"])
        .output()?;

    match output.status.success() {
        true => Ok(()),
        false => Err(anyhow!(String::from_utf8(output.stderr).unwrap())),
    }
}

impl VethDevice {
    pub fn peer(&self) -> Rc<VethDevice> {
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
