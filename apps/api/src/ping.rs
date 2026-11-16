//! Découverte réseau façon PHPIPAM : ping ICMP des adresses et lecture de la
//! table ARP pour associer automatiquement une adresse MAC aux IP qui répondent.
//!
//! - `ping_host` : un ping unique via le binaire système `ping` (aucun socket
//!   raw, donc pas besoin de privilèges élevés).
//! - `arp_table` / `parse_arp` : parse de `arp -an` (macOS et Linux).
//! - `sweep_subnet` : balayage à la demande d'un sous-réseau (bouton "scan").
//! - `background_sweep` : balayage périodique des IP déjà connues.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use ipnetwork::IpNetwork;
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::error::ApiError;
use crate::ipam_logic::{self, IpAddrNet};

/// Ping une adresse (1 paquet, timeout borné). Retourne true si réponse.
pub async fn ping_host(ip: IpAddr, timeout_ms: u64) -> bool {
    // -W : millisecondes sur macOS, secondes sur Linux.
    let wait = if cfg!(target_os = "macos") {
        timeout_ms.to_string()
    } else {
        ((timeout_ms / 1000).max(1)).to_string()
    };
    let status = tokio::process::Command::new("ping")
        .arg("-n") // pas de résolution inverse
        .arg("-c")
        .arg("1")
        .arg("-W")
        .arg(&wait)
        .arg(ip.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    matches!(status, Ok(s) if s.success())
}

/// Table ARP complète : vecteur (IP, MAC normalisée).
pub async fn arp_table() -> Vec<(IpAddr, String)> {
    let out = tokio::process::Command::new("arp")
        .arg("-an")
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            parse_arp(&text)
        }
        _ => Vec::new(),
    }
}

/// Parse la sortie de `arp -an` (macOS/Linux) :
/// `? (192.168.1.10) at aa:bb:cc:dd:ee:ff on en0 ifscope en0`
pub fn parse_arp(output: &str) -> Vec<(IpAddr, String)> {
    let mut out = Vec::new();
    for line in output.lines() {
        let Some(open) = line.find('(') else { continue };
        let Some(close) = line[open..].find(')').map(|i| open + i) else {
            continue;
        };
        let Ok(ip) = line[open + 1..close].trim().parse::<IpAddr>() else {
            continue;
        };
        let Some(at) = line.find(" at ") else {
            continue;
        };
        let rest = &line[at + 4..];
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if let Ok(mac) = ipam_logic::normalize_mac(&rest[..end]) {
            out.push((ip, mac));
        }
    }
    out
}

/// Itère les adresses utilisables d'un réseau (borné par `max`).
pub fn usable_addresses(net: &IpAddrNet, max: u128) -> Result<Vec<IpAddr>, ApiError> {
    let info = net.info();
    if info.usable_addresses > max {
        return Err(ApiError::Validation(format!(
            "trop d'adresses à balayer ({} > {max})",
            info.usable_addresses
        )));
    }
    let mut v = Vec::new();
    match (info.first_usable, info.last_usable) {
        (IpAddr::V4(first), IpAddr::V4(last)) => {
            let mut cur = u32::from(first);
            let end = u32::from(last);
            while cur <= end {
                v.push(IpAddr::V4(cur.into()));
                cur += 1;
            }
        }
        (IpAddr::V6(first), IpAddr::V6(last)) => {
            let mut cur = u128::from(first);
            let end = u128::from(last);
            while cur <= end {
                v.push(IpAddr::V6(cur.into()));
                cur += 1;
            }
        }
        _ => {}
    }
    Ok(v)
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct SweepResult {
    pub scanned: usize,
    pub online: usize,
    pub offline: usize,
    pub new_ips: usize,
    pub mac_filled: usize,
}

/// Ping une liste d'adresses avec concurrence bornée.
/// Retourne les adresses qui répondent, avec leur MAC ARP si connue.
async fn ping_many(
    addrs: Vec<IpAddr>,
    arp: &HashMap<IpAddr, String>,
    timeout_ms: u64,
    concurrency: usize,
) -> Vec<(IpAddr, Option<String>)> {
    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut js = JoinSet::new();
    for ip in addrs {
        let sem = sem.clone();
        let arp_mac = arp.get(&ip).cloned();
        js.spawn(async move {
            let _permit = sem.acquire().await;
            let ok = ping_host(ip, timeout_ms).await;
            (ip, ok, arp_mac)
        });
    }
    let mut online = Vec::new();
    while let Some(res) = js.join_next().await {
        if let Ok((ip, true, mac)) = res {
            online.push((ip, mac));
        }
    }
    online
}

/// Met à jour une IP après un ping positif : last_seen, statut offline -> allocated,
/// MAC remplie depuis l'ARP si vide.
async fn mark_online(
    pool: &PgPool,
    subnet_id: Uuid,
    ip: IpAddr,
    mac: Option<String>,
) -> Result<bool, ApiError> {
    let existing: Option<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT id, status::text, mac FROM ip_addresses WHERE subnet_id = $1 AND address = $2::inet",
    )
    .bind(subnet_id)
    .bind(IpNetwork::from(ip))
    .fetch_optional(pool)
    .await?;
    match existing {
        Some((id, status, cur_mac)) => {
            let new_status = if status == "offline" {
                "allocated"
            } else {
                status.as_str()
            };
            let mac_filled = cur_mac.is_none() && mac.is_some();
            sqlx::query(
                "UPDATE ip_addresses
                 SET last_ping_ok = TRUE, last_seen = now(), last_ping_at = now(),
                     status = $2::ip_status,
                     mac = COALESCE(mac, $3)
                 WHERE id = $1",
            )
            .bind(id)
            .bind(new_status)
            .bind(&mac)
            .execute(pool)
            .await?;
            Ok(mac_filled)
        }
        None => {
            // IP inconnue qui répond : on la découvre (statut dhcp, comme PHPIPAM).
            sqlx::query(
                "INSERT INTO ip_addresses (subnet_id, address, status, mac, last_ping_ok, last_seen, last_ping_at)
                 VALUES ($1, $2::inet, 'dhcp', $3, TRUE, now(), now())
                 ON CONFLICT (subnet_id, address) DO UPDATE
                   SET last_ping_ok = TRUE, last_seen = now(), last_ping_at = now()",
            )
            .bind(subnet_id)
            .bind(IpNetwork::from(ip))
            .bind(&mac)
            .execute(pool)
            .await?;
            Ok(mac.is_some())
        }
    }
}

/// Marque offline les IP allouées/dhcp du sous-réseau qui n'ont pas répondu.
async fn mark_offline(
    pool: &PgPool,
    subnet_id: Uuid,
    online: &[IpAddr],
) -> Result<usize, ApiError> {
    let addrs: Vec<IpNetwork> = online.iter().map(|i| IpNetwork::from(*i)).collect();
    let res = sqlx::query(
        "UPDATE ip_addresses
         SET status = 'offline', last_ping_ok = FALSE, last_ping_at = now()
         WHERE subnet_id = $1 AND status IN ('allocated', 'dhcp')
           AND NOT (address = ANY($2::inet[]))",
    )
    .bind(subnet_id)
    .bind(&addrs)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() as usize)
}

/// Scan à la demande d'un sous-réseau entier (bouton "Scanner").
pub async fn sweep_subnet(
    pool: &PgPool,
    subnet_id: Uuid,
    timeout_ms: u64,
    concurrency: usize,
    max_hosts: u128,
) -> Result<SweepResult, ApiError> {
    let (cidr,): (IpNetwork,) = sqlx::query_as("SELECT cidr FROM subnets WHERE id = $1")
        .bind(subnet_id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    let net = ipam_logic::parse_cidr(&cidr.to_string())?;
    let addrs = usable_addresses(&net, max_hosts)?;
    let scanned = addrs.len();
    // IP déjà connues AVANT le scan (pour compter les découvertes).
    let known: std::collections::HashSet<IpAddr> =
        sqlx::query_as::<_, (IpNetwork,)>("SELECT address FROM ip_addresses WHERE subnet_id = $1")
            .bind(subnet_id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|(n,)| n.ip())
            .collect();
    let arp: HashMap<IpAddr, String> = arp_table().await.into_iter().collect();
    let online = ping_many(addrs, &arp, timeout_ms, concurrency).await;
    let mut res = SweepResult {
        scanned,
        online: online.len(),
        ..Default::default()
    };
    let online_ips: Vec<IpAddr> = online.iter().map(|(ip, _)| *ip).collect();
    for (ip, mac) in &online {
        if mark_online(pool, subnet_id, *ip, mac.clone()).await? {
            res.mac_filled += 1;
        }
    }
    res.new_ips = online_ips.iter().filter(|ip| !known.contains(ip)).count();
    res.offline = mark_offline(pool, subnet_id, &online_ips).await?;
    Ok(res)
}

/// Balayage périodique : ne ping que les IP déjà enregistrées des sous-réseaux
/// dont `ping_scan` est actif. Appelé par la tâche de fond.
pub async fn background_sweep(pool: &PgPool, timeout_ms: u64, concurrency: usize) {
    let subnets: Vec<(Uuid,)> = match sqlx::query_as("SELECT id FROM subnets WHERE ping_scan")
        .fetch_all(pool)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "balayage ping : lecture des sous-réseaux");
            return;
        }
    };
    let arp: HashMap<IpAddr, String> = arp_table().await.into_iter().collect();
    for (subnet_id,) in subnets {
        let rows: Vec<(Uuid, IpNetwork, String)> = match sqlx::query_as(
            "SELECT id, address, status::text FROM ip_addresses
             WHERE subnet_id = $1 AND status <> 'free'",
        )
        .bind(subnet_id)
        .fetch_all(pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, subnet = %subnet_id, "balayage ping : lecture des IP");
                continue;
            }
        };
        let addrs: Vec<IpAddr> = rows.iter().map(|(_, n, _)| n.ip()).collect();
        let online = ping_many(addrs, &arp, timeout_ms, concurrency).await;
        let online_ips: Vec<IpAddr> = online.iter().map(|(ip, _)| *ip).collect();
        for (ip, mac) in &online {
            if let Err(e) = mark_online(pool, subnet_id, *ip, mac.clone()).await {
                tracing::error!(error = %e, ip = %ip, "balayage ping : mise à jour");
            }
        }
        if let Err(e) = mark_offline(pool, subnet_id, &online_ips).await {
            tracing::error!(error = %e, subnet = %subnet_id, "balayage ping : offline");
        }
    }
}

/// Tâche de fond : balayage toutes les `interval` secondes (0 = arrêtée).
pub async fn run_background_loop(
    pool: PgPool,
    interval: Duration,
    timeout_ms: u64,
    concurrency: usize,
) {
    if interval.is_zero() {
        tracing::info!("balayage ping automatique désactivé (PING_INTERVAL_SECONDS=0)");
        return;
    }
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        background_sweep(&pool, timeout_ms, concurrency).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_arp_macos() {
        let sample = "? (192.168.10.1) at aa:bb:cc:00:11:22 on en0 ifscope en0\n\
                      ? (192.168.10.2) at 11:22:33:44:55:66 on en0 ifscope en0\n\
                      ? (192.168.10.9) at (incomplete) on en0 ifscope en0\n\
                      ? (2001:db8::1) at aa:bb:cc:00:11:22 on en0 ifscope en0\n";
        let v = parse_arp(sample);
        assert_eq!(v.len(), 3);
        assert_eq!(
            v[0],
            (
                "192.168.10.1".parse::<IpAddr>().unwrap(),
                "AA:BB:CC:00:11:22".to_string()
            )
        );
        assert_eq!(
            v[2],
            (
                "2001:db8::1".parse::<IpAddr>().unwrap(),
                "AA:BB:CC:00:11:22".to_string()
            )
        );
    }

    #[test]
    fn usable_addresses_bounds() {
        let net = ipam_logic::parse_cidr("10.9.0.0/29").unwrap();
        let v = usable_addresses(&net, 100).unwrap();
        assert_eq!(v.len(), 6);
        assert_eq!(v[0], "10.9.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(v[5], "10.9.0.6".parse::<IpAddr>().unwrap());
        assert!(usable_addresses(&net, 5).is_err());
    }
}
