use std::{collections::HashSet, net::Ipv4Addr, str::FromStr};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct IpAllocator {
    network: Ipv4Addr,
    prefix: u8,
}

impl IpAllocator {
    pub const USABLE_ADDRESSES: usize = 253;

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

    pub fn available_slots(&self, used: usize) -> usize {
        Self::USABLE_ADDRESSES.saturating_sub(used)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_remaining_allocator_capacity() {
        let allocator = IpAllocator::new("10.8.0.0/24").unwrap();
        assert_eq!(allocator.available_slots(0), 253);
        assert_eq!(allocator.available_slots(252), 1);
        assert_eq!(allocator.available_slots(253), 0);
        assert_eq!(allocator.available_slots(300), 0);
    }
}
