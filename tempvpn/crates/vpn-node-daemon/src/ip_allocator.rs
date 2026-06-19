use std::{collections::HashSet, net::Ipv4Addr, str::FromStr};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct IpAllocator {
    network: Ipv4Addr,
    prefix: u8,
}

impl IpAllocator {
    pub fn new(cidr: &str) -> Result<Self> {
        let (addr, prefix) = cidr
            .split_once('/')
            .ok_or_else(|| Error::InvalidConfig(format!("tunnel_cidr must be CIDR, got {cidr}")))?;
        let network = Ipv4Addr::from_str(addr)?;
        let prefix = prefix.parse::<u8>()?;
        if prefix != 24 {
            return Err(Error::InvalidConfig(format!(
                "MVP IP allocator only supports /24, got /{prefix}"
            )));
        }
        Ok(Self { network, prefix })
    }

    pub fn allocate(&self, used: &HashSet<Ipv4Addr>) -> Option<Ipv4Addr> {
        let octets = self.network.octets();
        (2u8..=254)
            .map(|last| Ipv4Addr::new(octets[0], octets[1], octets[2], last))
            .find(|candidate| !used.contains(candidate))
    }

    pub fn peer_cidr(&self, ip: Ipv4Addr) -> String {
        let _ = self.prefix;
        format!("{ip}/32")
    }
}
