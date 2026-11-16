//! Logique IPAM pure (sans base de données) : calculs CIDR, validation,
//! prochaine IP libre. Testée unitairement ci-dessous.
//! ipnet 2.12 ne fournit plus size()/overlaps() : calculs manuels ici.

use ipnet::{Ipv4Net, Ipv6Net};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use uuid::Uuid;

use crate::error::ApiError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubnetInfo {
    pub network: IpAddr,
    pub broadcast: Option<IpAddr>, // None en IPv6
    pub first_usable: IpAddr,
    pub last_usable: IpAddr,
    pub total_addresses: u128,
    pub usable_addresses: u128,
}

fn v4_total(net: &Ipv4Net) -> u128 {
    1u128
        .checked_shl(u32::from(32 - net.prefix_len()))
        .unwrap_or(u128::MAX)
}

fn v6_total(net: &Ipv6Net) -> u128 {
    1u128
        .checked_shl(u32::from(128 - net.prefix_len()))
        .unwrap_or(u128::MAX)
}

fn v4_overlaps(a: &Ipv4Net, b: &Ipv4Net) -> bool {
    a.contains(&b.network()) || b.contains(&a.network())
}

fn v6_overlaps(a: &Ipv6Net, b: &Ipv6Net) -> bool {
    a.contains(&b.network()) || b.contains(&a.network())
}

pub fn subnet_info_v4(net: Ipv4Net) -> SubnetInfo {
    let network = net.network();
    let broadcast = net.broadcast();
    let total = v4_total(&net);
    // RFC 3021 : /31 et /32 ont des "usables" spéciaux ; on reste classique ailleurs.
    let (first_usable, last_usable, usable) = if net.prefix_len() >= 31 {
        (network, broadcast, total)
    } else {
        (
            Ipv4Addr::from(u32::from(network) + 1),
            Ipv4Addr::from(u32::from(broadcast) - 1),
            total.saturating_sub(2),
        )
    };
    SubnetInfo {
        network: IpAddr::V4(network),
        broadcast: Some(IpAddr::V4(broadcast)),
        first_usable: IpAddr::V4(first_usable),
        last_usable: IpAddr::V4(last_usable),
        total_addresses: total,
        usable_addresses: usable,
    }
}

pub fn subnet_info_v6(net: Ipv6Net) -> SubnetInfo {
    let network = net.network();
    let total = v6_total(&net);
    SubnetInfo {
        network: IpAddr::V6(network),
        broadcast: None,
        first_usable: IpAddr::V6(network),
        last_usable: IpAddr::V6(net.broadcast()),
        total_addresses: total,
        usable_addresses: total,
    }
}

pub fn parse_cidr(s: &str) -> Result<IpAddrNet, ApiError> {
    if let Ok(v4) = s.parse::<Ipv4Net>() {
        return Ok(IpAddrNet::V4(v4));
    }
    if let Ok(v6) = s.parse::<Ipv6Net>() {
        return Ok(IpAddrNet::V6(v6));
    }
    Err(ApiError::Validation(format!("CIDR invalide : {s}")))
}

#[derive(Clone, Copy, Debug)]
pub enum IpAddrNet {
    V4(Ipv4Net),
    V6(Ipv6Net),
}

impl IpAddrNet {
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self, ip) {
            (IpAddrNet::V4(n), IpAddr::V4(a)) => n.contains(&a),
            (IpAddrNet::V6(n), IpAddr::V6(a)) => n.contains(&a),
            _ => false,
        }
    }
    pub fn overlaps(&self, other: &IpAddrNet) -> bool {
        match (self, other) {
            (IpAddrNet::V4(a), IpAddrNet::V4(b)) => v4_overlaps(a, b),
            (IpAddrNet::V6(a), IpAddrNet::V6(b)) => v6_overlaps(a, b),
            _ => false,
        }
    }
    pub fn info(&self) -> SubnetInfo {
        match self {
            IpAddrNet::V4(n) => subnet_info_v4(*n),
            IpAddrNet::V6(n) => subnet_info_v6(*n),
        }
    }
    pub fn size(&self) -> u128 {
        match self {
            IpAddrNet::V4(n) => v4_total(n),
            IpAddrNet::V6(n) => v6_total(n),
        }
    }
}

/// Prochaine adresse utilisable libre dans le sous-réseau, sachant les
/// adresses déjà occupées. Ignore réseau/broadcast (sauf /31, /32 en v4).
pub fn next_free(net: &IpAddrNet, occupied: &[IpAddr]) -> Option<IpAddr> {
    let info = net.info();
    match (info.first_usable, info.last_usable) {
        (IpAddr::V4(first), IpAddr::V4(last)) => {
            let mut cur = u32::from(first);
            let end = u32::from(last);
            while cur <= end {
                let candidate = IpAddr::V4(Ipv4Addr::from(cur));
                if !occupied.contains(&candidate) {
                    return Some(candidate);
                }
                cur += 1;
            }
            None
        }
        (IpAddr::V6(first), IpAddr::V6(last)) => {
            let mut cur = u128::from(first);
            let end = u128::from(last);
            while cur <= end {
                let candidate = IpAddr::V6(Ipv6Addr::from(cur));
                if !occupied.contains(&candidate) {
                    return Some(candidate);
                }
                cur += 1;
            }
            None
        }
        _ => None,
    }
}

/// Valide qu'une gateway appartient bien au sous-réseau.
pub fn validate_gateway(net: &IpAddrNet, gateway: Option<IpAddr>) -> Result<(), ApiError> {
    if let Some(g) = gateway
        && !net.contains(g)
    {
        return Err(ApiError::Validation(
            "la passerelle doit appartenir au sous-réseau".into(),
        ));
    }
    Ok(())
}

/// Valide qu'un nouveau sous-réseau ne chevauche aucun existant dans le même
/// espace. `ignore_ids` exclut certains sous-réseaux (ex: soi-même en update,
/// ou le parent quand on imbrique un enfant dans un master).
pub fn validate_no_overlap(
    new: &IpAddrNet,
    existing: &[(Uuid, IpAddrNet)],
    ignore_ids: &[Uuid],
) -> Result<(), ApiError> {
    for (id, net) in existing {
        if ignore_ids.contains(id) {
            continue;
        }
        if new.overlaps(net) {
            return Err(ApiError::Conflict(format!(
                "chevauchement avec le sous-réseau existant {id}"
            )));
        }
    }
    Ok(())
}

pub fn normalize_mac(mac: &str) -> Result<String, ApiError> {
    let clean: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.len() != 12 {
        return Err(ApiError::Validation(
            "adresse MAC invalide (12 chiffres hexa)".into(),
        ));
    }
    let mut out = String::with_capacity(17);
    for (i, c) in clean.chars().enumerate() {
        if i > 0 && i % 2 == 0 {
            out.push(':');
        }
        out.push(c.to_ascii_uppercase());
    }
    Ok(out)
}

pub fn is_valid_hostname(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_v4_slash24() {
        let net = "192.168.10.0/24".parse::<Ipv4Net>().unwrap();
        let info = subnet_info_v4(net);
        assert_eq!(info.network, IpAddr::V4("192.168.10.0".parse().unwrap()));
        assert_eq!(
            info.broadcast,
            Some(IpAddr::V4("192.168.10.255".parse().unwrap()))
        );
        assert_eq!(
            info.first_usable,
            IpAddr::V4("192.168.10.1".parse().unwrap())
        );
        assert_eq!(
            info.last_usable,
            IpAddr::V4("192.168.10.254".parse().unwrap())
        );
        assert_eq!(info.total_addresses, 256);
        assert_eq!(info.usable_addresses, 254);
    }

    #[test]
    fn info_v4_slash31_rfc3021() {
        let net = "10.0.0.0/31".parse::<Ipv4Net>().unwrap();
        let info = subnet_info_v4(net);
        assert_eq!(info.usable_addresses, 2);
    }

    #[test]
    fn info_v4_slash32() {
        let net = "10.0.0.5/32".parse::<Ipv4Net>().unwrap();
        let info = subnet_info_v4(net);
        assert_eq!(info.usable_addresses, 1);
        assert_eq!(info.first_usable, IpAddr::V4("10.0.0.5".parse().unwrap()));
    }

    #[test]
    fn info_v6_slash64() {
        let net = "2001:db8::/64".parse::<Ipv6Net>().unwrap();
        let info = subnet_info_v6(net);
        assert_eq!(info.broadcast, None);
        assert_eq!(info.network, IpAddr::V6("2001:db8::".parse().unwrap()));
        assert!(info.usable_addresses > u128::from(u64::MAX));
    }

    #[test]
    fn next_free_skips_occupied_and_reserved_edges() {
        let net = IpAddrNet::V4("10.1.0.0/29".parse().unwrap());
        let occupied = vec![
            IpAddr::V4("10.1.0.1".parse().unwrap()), // gateway
            IpAddr::V4("10.1.0.2".parse().unwrap()),
        ];
        assert_eq!(
            next_free(&net, &occupied),
            Some(IpAddr::V4("10.1.0.3".parse().unwrap()))
        );
        let full = vec![
            "10.1.0.1", "10.1.0.2", "10.1.0.3", "10.1.0.4", "10.1.0.5", "10.1.0.6",
        ]
        .into_iter()
        .map(|s| IpAddr::V4(s.parse().unwrap()))
        .collect::<Vec<_>>();
        assert_eq!(next_free(&net, &full), None);
    }

    #[test]
    fn overlap_detection() {
        let a = IpAddrNet::V4("10.0.0.0/8".parse().unwrap());
        let b = IpAddrNet::V4("10.1.0.0/16".parse().unwrap());
        let c = IpAddrNet::V4("192.168.0.0/16".parse().unwrap());
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
        assert!(
            !IpAddrNet::V4("10.0.0.0/8".parse().unwrap())
                .overlaps(&IpAddrNet::V6("2001:db8::/32".parse().unwrap()))
        );
    }

    #[test]
    fn gateway_must_be_in_subnet() {
        let net = IpAddrNet::V4("10.0.0.0/24".parse().unwrap());
        assert!(validate_gateway(&net, Some(IpAddr::V4("10.0.0.1".parse().unwrap()))).is_ok());
        assert!(validate_gateway(&net, Some(IpAddr::V4("10.0.1.1".parse().unwrap()))).is_err());
        assert!(validate_gateway(&net, None).is_ok());
    }

    #[test]
    fn mac_normalization() {
        assert_eq!(
            normalize_mac("aa:bb:cc:dd:ee:ff").unwrap(),
            "AA:BB:CC:DD:EE:FF"
        );
        assert_eq!(normalize_mac("aabbccddeeff").unwrap(), "AA:BB:CC:DD:EE:FF");
        assert!(normalize_mac("aa:bb").is_err());
        assert!(normalize_mac("zz:bb:cc:dd:ee:ff").is_err());
    }

    #[test]
    fn hostname_validation() {
        assert!(is_valid_hostname("srv-web-01.example.com"));
        assert!(is_valid_hostname("pc-42"));
        assert!(!is_valid_hostname("-bad.example.com"));
        assert!(!is_valid_hostname("bad-.example.com"));
        assert!(!is_valid_hostname(""));
        assert!(!is_valid_hostname("ho_st.example.com"));
    }

    #[test]
    fn overlap_check_with_ignore() {
        let id = Uuid::new_v4();
        let master = Uuid::new_v4();
        let existing = vec![
            (id, IpAddrNet::V4("10.0.0.0/24".parse().unwrap())),
            (master, IpAddrNet::V4("10.0.0.0/16".parse().unwrap())),
        ];
        let same = IpAddrNet::V4("10.0.0.0/24".parse().unwrap());
        // /24 chevauche aussi le master /16 : les deux doivent être ignorés
        assert!(validate_no_overlap(&same, &existing, &[id, master]).is_ok());
        assert!(validate_no_overlap(&same, &existing, &[id]).is_err());
        assert!(validate_no_overlap(&same, &existing, &[]).is_err());
        // enfant imbriqué dans le master : le master est ignoré, pas l'autre
        let child = IpAddrNet::V4("10.0.0.0/25".parse().unwrap());
        assert!(validate_no_overlap(&child, &existing, &[master]).is_err());
        assert!(validate_no_overlap(&child, &existing, &[master, id]).is_ok());
        let other = IpAddrNet::V4("10.1.0.0/24".parse().unwrap());
        assert!(validate_no_overlap(&other, &existing, &[]).is_ok());
    }
}
