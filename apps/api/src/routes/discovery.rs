//! Routes de découverte réseau : scan d'un sous-réseau (ping + ARP) et
//! ping individuel d'une IP. Le balayage périodique vit dans `ping.rs`.

use axum::{
    Json,
    extract::{Path, State},
};
use ipnetwork::IpNetwork;
use uuid::Uuid;

use crate::{
    audit,
    auth::{Claims, Role, require_role},
    error::{ApiError, ApiResult},
    models::ScanResultDto,
    ping,
    state::AppState,
};

/// POST /ipam/subnets/{id}/scan — balaie toutes les adresses utilisables du
/// sous-réseau : les IP qui répondent sont marquées vues (statut dhcp si
/// inconnues, MAC remplie depuis l'ARP), les autres passent offline.
pub async fn scan_subnet(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ScanResultDto>> {
    require_role(&claims, Role::Operator)?;
    let res = ping::sweep_subnet(
        &state.pool,
        id,
        state.ping_timeout_ms,
        state.ping_concurrency,
        state.scan_max_hosts,
    )
    .await?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.subnet.scan",
        "subnet",
        Some(id.to_string()),
    )
    .details(serde_json::json!({
        "scanned": res.scanned, "online": res.online, "offline": res.offline,
        "new_ips": res.new_ips, "mac_filled": res.mac_filled,
    }))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(ScanResultDto {
        scanned: res.scanned,
        online: res.online,
        offline: res.offline,
        new_ips: res.new_ips,
        mac_filled: res.mac_filled,
    }))
}

/// POST /ipam/ips/{id}/ping — ping une IP et met à jour last_ping/last_seen.
pub async fn ping_ip(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_role(&claims, Role::Operator)?;
    let (addr,): (IpNetwork,) = sqlx::query_as("SELECT address FROM ip_addresses WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    let ip = addr.ip();
    let ok = ping::ping_host(ip, state.ping_timeout_ms).await;
    let mac = if ok {
        ping::arp_table()
            .await
            .into_iter()
            .find(|(a, _)| *a == ip)
            .map(|(_, m)| m)
    } else {
        None
    };
    let status: Option<(String,)> =
        sqlx::query_as("SELECT status::text FROM ip_addresses WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let new_status = match status.as_ref().map(|s| s.0.as_str()) {
        Some("offline") if ok => "allocated",
        Some("allocated") | Some("dhcp") if !ok => "offline",
        other => other.unwrap_or("allocated"),
    };
    sqlx::query(
        "UPDATE ip_addresses
         SET last_ping_ok = $2, last_ping_at = now(),
             last_seen = CASE WHEN $2 THEN now() ELSE last_seen END,
             status = $3::ip_status,
             mac = COALESCE(mac, $4)
         WHERE id = $1",
    )
    .bind(id)
    .bind(ok)
    .bind(new_status)
    .bind(&mac)
    .execute(&state.pool)
    .await?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.ip.ping",
        "ip_address",
        Some(id.to_string()),
    )
    .details(serde_json::json!({"address": ip.to_string(), "ok": ok}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(serde_json::json!({
        "address": ip.to_string(),
        "online": ok,
        "mac": mac,
        "status": new_status,
    })))
}
