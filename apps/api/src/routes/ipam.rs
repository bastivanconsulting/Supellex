use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use ipnetwork::IpNetwork;
use std::net::IpAddr;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

use crate::{
    audit,
    auth::{Claims, Role, require_role},
    error::{ApiError, ApiResult},
    ipam_logic::{self, IpAddrNet},
    models::*,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/spaces", spaces_router())
        .nest("/vlans", vlans_router())
        .nest("/subnets", subnets_router())
        .nest("/ips", ips_router())
}

// ---------------- Spaces ----------------

fn spaces_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_spaces).post(create_space))
        .route(
            "/{id}",
            get(get_space).put(update_space).delete(delete_space),
        )
}

async fn list_spaces(
    State(state): State<AppState>,
    _c: Claims,
) -> ApiResult<Json<Vec<IpSpaceDto>>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
        "SELECT id, name, description FROM ip_spaces ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, description)| IpSpaceDto {
                id,
                name,
                description,
            })
            .collect(),
    ))
}

async fn get_space(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<IpSpaceDto>> {
    let row = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
        "SELECT id, name, description FROM ip_spaces WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(IpSpaceDto {
        id: row.0,
        name: row.1,
        description: row.2,
    }))
}

async fn create_space(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertSpaceRequest>,
) -> ApiResult<Json<IpSpaceDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let row = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
        "INSERT INTO ip_spaces (name, description) VALUES ($1, $2) RETURNING id, name, description",
    )
    .bind(&req.name)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("existe déjà".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.space.create",
        "ip_space",
        Some(row.0.to_string()),
    )
    .details(serde_json::json!({"name": req.name}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(IpSpaceDto {
        id: row.0,
        name: row.1,
        description: row.2,
    }))
}

async fn update_space(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertSpaceRequest>,
) -> ApiResult<Json<IpSpaceDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let row = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
        "UPDATE ip_spaces SET name=$2, description=$3 WHERE id=$1 RETURNING id, name, description",
    )
    .bind(id)
    .bind(&req.name)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.space.update",
        "ip_space",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(IpSpaceDto {
        id: row.0,
        name: row.1,
        description: row.2,
    }))
}

async fn delete_space(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM ip_spaces WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.space.delete",
        "ip_space",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------- VLANs ----------------

fn vlans_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_vlans).post(create_vlan))
        .route("/{id}", get(get_vlan).put(update_vlan).delete(delete_vlan))
        .route("/{id}/subnets", get(list_subnets_of_vlan))
}

type VlanRow = (Uuid, Uuid, i32, String, Option<String>);

async fn vlan_dto(state: &AppState, r: VlanRow) -> VlanDto {
    let (subnets_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM subnets WHERE vlan_id = $1")
            .bind(r.0)
            .fetch_one(&state.pool)
            .await
            .unwrap_or((0,));
    let (used_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM ip_addresses ip JOIN subnets s ON s.id = ip.subnet_id
         WHERE s.vlan_id = $1 AND ip.status <> 'free'",
    )
    .bind(r.0)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));
    VlanDto {
        id: r.0,
        vlan_domain_id: r.1,
        vid: r.2,
        name: r.3,
        description: r.4,
        subnets_count,
        used_count,
    }
}

async fn list_vlans(State(state): State<AppState>, _c: Claims) -> ApiResult<Json<Vec<VlanDto>>> {
    let rows = sqlx::query_as::<_, VlanRow>(
        "SELECT id, vlan_domain_id, vid, name, description FROM vlans ORDER BY vid",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(vlan_dto(&state, r).await);
    }
    Ok(Json(out))
}

async fn get_vlan(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<VlanDto>> {
    let row = sqlx::query_as::<_, VlanRow>(
        "SELECT id, vlan_domain_id, vid, name, description FROM vlans WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(vlan_dto(&state, row).await))
}

/// Sous-réseaux rattachés à un VLAN.
async fn list_subnets_of_vlan(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let rows = sqlx::query_as::<_, (Uuid, IpNetwork, Option<String>)>(
        "SELECT id, cidr, name FROM subnets WHERE vlan_id = $1 ORDER BY cidr",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, cidr, name)| {
                serde_json::json!({ "id": id, "cidr": cidr.to_string(), "name": name })
            })
            .collect(),
    ))
}

async fn create_vlan(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertVlanRequest>,
) -> ApiResult<Json<VlanDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let domain_id = match req.vlan_domain_id {
        Some(d) => d,
        None => crate::routes::catalog::default_domain_id(&state).await?,
    };
    let row = sqlx::query_as::<_, VlanRow>(
        "INSERT INTO vlans (vlan_domain_id, vid, name, description) VALUES ($1, $2, $3, $4)
         RETURNING id, vlan_domain_id, vid, name, description",
    )
    .bind(domain_id)
    .bind(req.vid)
    .bind(&req.name)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("existe déjà".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.vlan.create",
        "vlan",
        Some(row.0.to_string()),
    )
    .details(serde_json::json!({"vid": req.vid, "name": req.name}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(vlan_dto(&state, row).await))
}

async fn update_vlan(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertVlanRequest>,
) -> ApiResult<Json<VlanDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let domain_id = match req.vlan_domain_id {
        Some(d) => d,
        None => crate::routes::catalog::default_domain_id(&state).await?,
    };
    let row = sqlx::query_as::<_, VlanRow>(
        "UPDATE vlans SET vlan_domain_id=$2, vid=$3, name=$4, description=$5 WHERE id=$1
         RETURNING id, vlan_domain_id, vid, name, description",
    )
    .bind(id)
    .bind(domain_id)
    .bind(req.vid)
    .bind(&req.name)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.vlan.update",
        "vlan",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(vlan_dto(&state, row).await))
}

async fn delete_vlan(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM vlans WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.vlan.delete",
        "vlan",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------- Subnets ----------------

fn subnets_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_subnets).post(create_subnet))
        .route(
            "/{id}",
            get(get_subnet).put(update_subnet).delete(delete_subnet),
        )
        .route("/{id}/ips", get(list_ips_of_subnet).post(allocate_ip))
        .route("/{id}/next-free", get(next_free_ip))
        .route("/{id}/children", get(list_subnet_children))
        .route(
            "/{id}/scan",
            axum::routing::post(crate::routes::discovery::scan_subnet),
        )
}

type SubnetRow = (
    Uuid,
    Uuid,
    IpNetwork,
    Option<String>,
    Option<String>,
    Option<IpNetwork>,
    Option<Uuid>,
    Option<Uuid>,
    Option<Vec<IpNetwork>>,
    bool,
    Option<Uuid>,
    bool,
    serde_json::Value,
    chrono::DateTime<chrono::Utc>,
);

async fn subnet_dto(state: &AppState, r: SubnetRow) -> SubnetDto {
    let net = ipam_logic::parse_cidr(&r.2.to_string())
        .unwrap_or(IpAddrNet::V4("0.0.0.0/32".parse().unwrap()));
    let info = net.info();
    let counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status::text, COUNT(*) FROM ip_addresses WHERE subnet_id = $1 GROUP BY status::text",
    )
    .bind(r.0)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    let get = |s: &str| {
        counts
            .iter()
            .find(|(k, _)| k == s)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    };
    let used = counts.iter().map(|(_, c)| c).sum::<i64>() - get("free");
    let (children_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM subnets WHERE master_subnet_id = $1")
            .bind(r.0)
            .fetch_one(&state.pool)
            .await
            .unwrap_or((0,));
    let utilization_pct = if info.usable_addresses > 0 {
        (used as f64 / info.usable_addresses as f64) * 100.0
    } else {
        0.0
    };
    SubnetDto {
        id: r.0,
        space_id: r.1,
        cidr: r.2.to_string(),
        name: r.3,
        description: r.4,
        gateway: r.5.map(|g| g.to_string()),
        vlan_id: r.6,
        location_id: r.7,
        dns_servers: r
            .8
            .unwrap_or_default()
            .iter()
            .map(|d| d.to_string())
            .collect(),
        is_pool: r.9,
        master_subnet_id: r.10,
        ping_scan: r.11,
        custom_fields: r.12,
        total_addresses: info.total_addresses.to_string(),
        usable_addresses: info.usable_addresses.to_string(),
        used_count: used,
        free_count: get("free"),
        allocated_count: get("allocated"),
        reserved_count: get("reserved"),
        dhcp_count: get("dhcp"),
        offline_count: get("offline"),
        children_count,
        utilization_pct: (utilization_pct * 10.0).round() / 10.0,
        created_at: r.13,
    }
}

async fn list_subnets(
    State(state): State<AppState>,
    _c: Claims,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Paged<SubnetDto>>> {
    let filter = q.q.clone().unwrap_or_default();
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM subnets WHERE ($1 = '' OR (cidr::text || ' ' || COALESCE(name,'')) ILIKE '%' || $1 || '%')",
    )
    .bind(&filter)
    .fetch_one(&state.pool).await?;
    let rows = sqlx::query_as::<_, SubnetRow>(
        "SELECT id, space_id, cidr, name, description, gateway, vlan_id, location_id, dns_servers, is_pool, master_subnet_id, ping_scan, custom_fields, created_at
         FROM subnets
         WHERE ($3 = '' OR (cidr::text || ' ' || COALESCE(name,'')) ILIKE '%' || $3 || '%')
         ORDER BY cidr LIMIT $1 OFFSET $2",
    )
    .bind(q.limit()).bind(q.offset()).bind(&filter)
    .fetch_all(&state.pool).await?;
    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        items.push(subnet_dto(&state, r).await);
    }
    Ok(Json(Paged {
        items,
        total: total.0,
        limit: q.limit(),
        offset: q.offset(),
    }))
}

async fn get_subnet(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SubnetDto>> {
    let row = sqlx::query_as::<_, SubnetRow>(
        "SELECT id, space_id, cidr, name, description, gateway, vlan_id, location_id, dns_servers, is_pool, master_subnet_id, ping_scan, custom_fields, created_at
         FROM subnets WHERE id = $1",
    )
    .bind(id).fetch_optional(&state.pool).await?.ok_or(ApiError::NotFound)?;
    Ok(Json(subnet_dto(&state, row).await))
}

async fn create_subnet(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertSubnetRequest>,
) -> ApiResult<Json<SubnetDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let net = ipam_logic::parse_cidr(&req.cidr)?;
    if net.size() > 65_536 {
        return Err(ApiError::Validation(
            "sous-réseau trop grand (max /16 en v4)".into(),
        ));
    }
    let gateway = match &req.gateway {
        Some(g) => {
            Some(IpAddr::from_str(g).map_err(|_| ApiError::Validation("gateway invalide".into()))?)
        }
        None => None,
    };
    ipam_logic::validate_gateway(&net, gateway)?;
    // Pas de chevauchement dans le même espace
    let existing =
        sqlx::query_as::<_, (Uuid, IpNetwork)>("SELECT id, cidr FROM subnets WHERE space_id = $1")
            .bind(req.space_id)
            .fetch_all(&state.pool)
            .await?;
    let existing_nets: Vec<(Uuid, IpAddrNet)> = existing
        .into_iter()
        .filter_map(|(id, n)| ipam_logic::parse_cidr(&n.to_string()).ok().map(|x| (id, x)))
        .collect();
    // En imbrication, le parent est exclu du contrôle de chevauchement.
    let ignore = req.master_subnet_id.into_iter().collect::<Vec<_>>();
    ipam_logic::validate_no_overlap(&net, &existing_nets, &ignore)?;
    validate_master(&state, &net, req.space_id, req.master_subnet_id).await?;
    if let Some(cf) = &req.custom_fields {
        crate::routes::catalog::validate_custom_fields(&state.pool, "subnet", cf).await?;
    }
    let dns: Vec<IpNetwork> = req
        .dns_servers
        .unwrap_or_default()
        .iter()
        .map(|s| {
            IpNetwork::from_str(s).map_err(|_| ApiError::Validation(format!("DNS invalide : {s}")))
        })
        .collect::<Result<_, _>>()?;
    let row = sqlx::query_as::<_, SubnetRow>(
        "INSERT INTO subnets (space_id, cidr, name, description, gateway, vlan_id, location_id, dns_servers, is_pool, master_subnet_id, ping_scan, custom_fields)
         VALUES ($1, $2::inet, $3, $4, $5::inet, $6, $7, $8, $9, $10, $11, $12)
         RETURNING id, space_id, cidr, name, description, gateway, vlan_id, location_id, dns_servers, is_pool, master_subnet_id, ping_scan, custom_fields, created_at",
    )
    .bind(req.space_id)
    .bind(req.cidr.clone())
    .bind(&req.name)
    .bind(&req.description)
    .bind(gateway.map(IpNetwork::from))
    .bind(req.vlan_id)
    .bind(req.location_id)
    .bind(&dns)
    .bind(req.is_pool.unwrap_or(false))
    .bind(req.master_subnet_id)
    .bind(req.ping_scan.unwrap_or(true))
    .bind(req.custom_fields.clone().unwrap_or(serde_json::json!({})))
    .fetch_optional(&state.pool).await?.ok_or(ApiError::Conflict("existe déjà".into()))?;
    // Le gateway occupe une IP marquée 'gateway'
    if let Some(g) = gateway {
        sqlx::query("INSERT INTO ip_addresses (subnet_id, address, status) VALUES ($1, $2::inet, 'gateway') ON CONFLICT DO NOTHING")
            .bind(row.0).bind(IpNetwork::from(g)).execute(&state.pool).await?;
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.subnet.create",
        "subnet",
        Some(row.0.to_string()),
    )
    .details(serde_json::json!({"cidr": req.cidr}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(subnet_dto(&state, row).await))
}

async fn update_subnet(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertSubnetRequest>,
) -> ApiResult<Json<SubnetDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let net = ipam_logic::parse_cidr(&req.cidr)?;
    let gateway = match &req.gateway {
        Some(g) => {
            Some(IpAddr::from_str(g).map_err(|_| ApiError::Validation("gateway invalide".into()))?)
        }
        None => None,
    };
    ipam_logic::validate_gateway(&net, gateway)?;
    let existing =
        sqlx::query_as::<_, (Uuid, IpNetwork)>("SELECT id, cidr FROM subnets WHERE space_id = $1")
            .bind(req.space_id)
            .fetch_all(&state.pool)
            .await?;
    let existing_nets: Vec<(Uuid, IpAddrNet)> = existing
        .into_iter()
        .filter_map(|(i, n)| ipam_logic::parse_cidr(&n.to_string()).ok().map(|x| (i, x)))
        .collect();
    let mut ignore = vec![id];
    if let Some(m) = req.master_subnet_id
        && m != id
    {
        ignore.push(m);
    }
    ipam_logic::validate_no_overlap(&net, &existing_nets, &ignore)?;
    validate_master(&state, &net, req.space_id, req.master_subnet_id).await?;
    if let Some(cf) = &req.custom_fields {
        crate::routes::catalog::validate_custom_fields(&state.pool, "subnet", cf).await?;
    }
    let dns: Vec<IpNetwork> = req
        .dns_servers
        .unwrap_or_default()
        .iter()
        .map(|s| {
            IpNetwork::from_str(s).map_err(|_| ApiError::Validation(format!("DNS invalide : {s}")))
        })
        .collect::<Result<_, _>>()?;
    let row = sqlx::query_as::<_, SubnetRow>(
        "UPDATE subnets SET space_id=$2, cidr=$3::inet, name=$4, description=$5, gateway=$6::inet, vlan_id=$7,
           location_id=$8, dns_servers=$9, is_pool=$10, master_subnet_id=$11, ping_scan=$12, custom_fields=$13 WHERE id=$1
         RETURNING id, space_id, cidr, name, description, gateway, vlan_id, location_id, dns_servers, is_pool, master_subnet_id, ping_scan, custom_fields, created_at",
    )
    .bind(id)
    .bind(req.space_id)
    .bind(req.cidr.clone())
    .bind(&req.name)
    .bind(&req.description)
    .bind(gateway.map(IpNetwork::from))
    .bind(req.vlan_id)
    .bind(req.location_id)
    .bind(&dns)
    .bind(req.is_pool.unwrap_or(false))
    .bind(req.master_subnet_id)
    .bind(req.ping_scan.unwrap_or(true))
    .bind(req.custom_fields.clone().unwrap_or(serde_json::json!({})))
    .fetch_optional(&state.pool).await?.ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.subnet.update",
        "subnet",
        Some(id.to_string()),
    )
    .details(serde_json::json!({"cidr": req.cidr}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(subnet_dto(&state, row).await))
}

async fn delete_subnet(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM subnets WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.subnet.delete",
        "subnet",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Valide le sous-réseau parent : même espace et le parent englobe l'enfant.
async fn validate_master(
    state: &AppState,
    net: &IpAddrNet,
    space_id: Uuid,
    master_id: Option<Uuid>,
) -> Result<(), ApiError> {
    let Some(master_id) = master_id else {
        return Ok(());
    };
    let row: Option<(IpNetwork, Uuid)> =
        sqlx::query_as("SELECT cidr, space_id FROM subnets WHERE id = $1")
            .bind(master_id)
            .fetch_optional(&state.pool)
            .await?;
    let (mcidr, mspace) =
        row.ok_or_else(|| ApiError::Validation("sous-réseau parent introuvable".into()))?;
    if mspace != space_id {
        return Err(ApiError::Validation(
            "le sous-réseau parent doit appartenir au même espace".into(),
        ));
    }
    let mnet = ipam_logic::parse_cidr(&mcidr.to_string())?;
    let info = net.info();
    if !mnet.contains(info.network) || !mnet.contains(info.last_usable) {
        return Err(ApiError::Validation(
            "le sous-réseau doit être entièrement contenu dans son parent".into(),
        ));
    }
    Ok(())
}

/// Sous-réseaux enfants (imbrication).
async fn list_subnet_children(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<SubnetDto>>> {
    let rows = sqlx::query_as::<_, SubnetRow>(
        "SELECT id, space_id, cidr, name, description, gateway, vlan_id, location_id, dns_servers, is_pool, master_subnet_id, ping_scan, custom_fields, created_at
         FROM subnets WHERE master_subnet_id = $1 ORDER BY cidr",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(subnet_dto(&state, r).await);
    }
    Ok(Json(out))
}

// ---------------- IPs ----------------

fn ips_router() -> Router<AppState> {
    Router::new()
        .route("/{id}", get(get_ip).put(update_ip).delete(delete_ip))
        .route(
            "/{id}/ping",
            axum::routing::post(crate::routes::discovery::ping_ip),
        )
        .route(
            "/{id}/tags",
            axum::routing::put(crate::routes::catalog::set_ip_tags),
        )
}

type IpRow = (
    Uuid,
    Uuid,
    IpNetwork,
    String,
    Option<String>,
    Option<String>,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    serde_json::Value,
    Option<bool>,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
    chrono::DateTime<chrono::Utc>,
);

async fn ip_dto(state: &AppState, r: IpRow) -> IpAddressDto {
    IpAddressDto {
        id: r.0,
        subnet_id: r.1,
        address: r.2.ip().to_string(),
        status: IpStatus::from_db(&r.3).unwrap_or(IpStatus::Free),
        hostname: r.4,
        mac: r.5,
        asset_id: r.6,
        person_id: r.7,
        description: r.8,
        custom_fields: r.9,
        tags: crate::routes::catalog::tags_of_ip(&state.pool, r.0).await,
        last_ping_ok: r.10,
        last_ping_at: r.11,
        last_seen: r.12,
        updated_at: r.13,
    }
}

macro_rules! ip_query {
    ($tail:literal) => {
        concat!("SELECT id, subnet_id, address, status::text, hostname, mac, asset_id, person_id, description, custom_fields, last_ping_ok, last_ping_at, last_seen, updated_at FROM ip_addresses ", $tail)
    };
}

/// Refuse une MAC déjà associée à une autre IP active (évite les doublons L2).
async fn check_mac_conflict(
    pool: &sqlx::PgPool,
    mac: &str,
    exclude: Option<Uuid>,
) -> Result<(), ApiError> {
    let row: Option<(Uuid, IpNetwork)> = sqlx::query_as(
        "SELECT id, address FROM ip_addresses
         WHERE mac = $1 AND status <> 'free' AND ($2::uuid IS NULL OR id <> $2) LIMIT 1",
    )
    .bind(mac)
    .bind(exclude)
    .fetch_optional(pool)
    .await?;
    match row {
        Some((_, addr)) => Err(ApiError::Conflict(format!(
            "MAC déjà associée à l'IP {}",
            addr.ip()
        ))),
        None => Ok(()),
    }
}

async fn list_ips_of_subnet(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Paged<IpAddressDto>>> {
    let filter = q.q.clone().unwrap_or_default();
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM ip_addresses
         WHERE subnet_id = $3 AND ($4 = '' OR (address::text || ' ' || COALESCE(hostname,'')) ILIKE '%' || $4 || '%')",
    )
    .bind(q.limit()).bind(q.offset()).bind(id).bind(&filter)
    .fetch_one(&state.pool).await?;
    let rows = sqlx::query_as::<_, IpRow>(
        "SELECT id, subnet_id, address, status::text, hostname, mac, asset_id, person_id, description, custom_fields, last_ping_ok, last_ping_at, last_seen, updated_at
         FROM ip_addresses
         WHERE subnet_id = $3 AND ($4 = '' OR (address::text || ' ' || COALESCE(hostname,'') || ' ' || COALESCE(mac,'')) ILIKE '%' || $4 || '%')
         ORDER BY address LIMIT $1 OFFSET $2",
    )
    .bind(q.limit()).bind(q.offset()).bind(id).bind(&filter)
    .fetch_all(&state.pool).await?;
    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        items.push(ip_dto(&state, r).await);
    }
    Ok(Json(Paged {
        items,
        total: total.0,
        limit: q.limit(),
        offset: q.offset(),
    }))
}

async fn get_ip(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<IpAddressDto>> {
    let row = sqlx::query_as::<_, IpRow>(ip_query!("WHERE id = $1"))
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(ip_dto(&state, row).await))
}

/// Allocation d'une IP dans un sous-réseau. Si `address` est absent,
/// la prochaine IP libre est calculée automatiquement.
async fn allocate_ip(
    State(state): State<AppState>,
    claims: Claims,
    Path(subnet_id): Path<Uuid>,
    Json(req): Json<AllocateIpRequest>,
) -> ApiResult<Json<IpAddressDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if let Some(h) = &req.hostname
        && !ipam_logic::is_valid_hostname(h)
    {
        return Err(ApiError::Validation("hostname invalide".into()));
    }
    let mac = req
        .mac
        .as_deref()
        .map(ipam_logic::normalize_mac)
        .transpose()?;

    let (cidr,): (IpNetwork,) = sqlx::query_as("SELECT cidr FROM subnets WHERE id = $1")
        .bind(subnet_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    let net = ipam_logic::parse_cidr(&cidr.to_string())?;

    let address = match &req.address {
        Some(a) => {
            let ip = IpAddr::from_str(a)
                .map_err(|_| ApiError::Validation("adresse IP invalide".into()))?;
            if !net.contains(ip) {
                return Err(ApiError::Validation(
                    "l'adresse n'appartient pas à ce sous-réseau".into(),
                ));
            }
            ip
        }
        None => {
            let occupied: Vec<IpAddr> = sqlx::query_as::<_, (IpNetwork,)>(
                "SELECT address FROM ip_addresses WHERE subnet_id = $1",
            )
            .bind(subnet_id)
            .fetch_all(&state.pool)
            .await?
            .into_iter()
            .map(|(n,)| n.ip())
            .collect();
            ipam_logic::next_free(&net, &occupied)
                .ok_or_else(|| ApiError::Conflict("sous-réseau plein".into()))?
        }
    };

    if let Some(m) = &mac {
        check_mac_conflict(&state.pool, m, None).await?;
    }
    if let Some(cf) = &req.custom_fields {
        crate::routes::catalog::validate_custom_fields(&state.pool, "ip_address", cf).await?;
    }
    let status = req.status.unwrap_or(IpStatus::Allocated);
    let row = sqlx::query_as::<_, IpRow>(
        "INSERT INTO ip_addresses (subnet_id, address, status, hostname, mac, asset_id, person_id, description, custom_fields)
         VALUES ($1, $2::inet, $3::ip_status, $4, $5, $6, $7, $8, $9)
         RETURNING id, subnet_id, address, status::text, hostname, mac, asset_id, person_id, description, custom_fields, last_ping_ok, last_ping_at, last_seen, updated_at",
    )
    .bind(subnet_id)
    .bind(IpNetwork::from(address))
    .bind(status.as_str())
    .bind(&req.hostname)
    .bind(&mac)
    .bind(req.asset_id)
    .bind(req.person_id)
    .bind(&req.description)
    .bind(req.custom_fields.clone().unwrap_or(serde_json::json!({})))
    .fetch_optional(&state.pool).await?;
    let row = match row {
        Some(r) => r,
        None => {
            // Course avec un autre writer : l'IP est peut-être prise entre-temps.
            return Err(ApiError::Conflict("existe déjà".into()));
        }
    };
    if let Some(ids) = &req.tag_ids {
        for t in ids {
            sqlx::query(
                "INSERT INTO ip_tags (ip_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(row.0)
            .bind(t)
            .execute(&state.pool)
            .await?;
        }
    }
    audit::AuditEvent::new(Some(&claims), "ipam.ip.allocate", "ip_address", Some(row.0.to_string())).details(serde_json::json!({"subnet_id": subnet_id, "address": address.to_string(), "status": status.as_str(), "mac": mac})).ip(None).write(&state.pool).await;
    Ok(Json(ip_dto(&state, row).await))
}

async fn update_ip(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateIpRequest>,
) -> ApiResult<Json<IpAddressDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let mac = req
        .mac
        .as_deref()
        .map(ipam_logic::normalize_mac)
        .transpose()?;
    if let Some(m) = &mac {
        check_mac_conflict(&state.pool, m, Some(id)).await?;
    }
    if let Some(h) = &req.hostname
        && !ipam_logic::is_valid_hostname(h)
    {
        return Err(ApiError::Validation("hostname invalide".into()));
    }
    if let Some(cf) = &req.custom_fields {
        crate::routes::catalog::validate_custom_fields(&state.pool, "ip_address", cf).await?;
    }
    let row = sqlx::query_as::<_, IpRow>(
        "UPDATE ip_addresses SET
           status = COALESCE($2::ip_status, status),
           hostname = COALESCE($3, hostname),
           mac = COALESCE($4, mac),
           asset_id = COALESCE($5, asset_id),
           person_id = COALESCE($6, person_id),
           description = COALESCE($7, description),
           custom_fields = COALESCE($8, custom_fields)
         WHERE id = $1
         RETURNING id, subnet_id, address, status::text, hostname, mac, asset_id, person_id, description, custom_fields, last_ping_ok, last_ping_at, last_seen, updated_at",
    )
    .bind(id)
    .bind(req.status.map(|s| s.as_str().to_string()))
    .bind(&req.hostname)
    .bind(&mac)
    .bind(req.asset_id)
    .bind(req.person_id)
    .bind(&req.description)
    .bind(&req.custom_fields)
    .fetch_optional(&state.pool).await?.ok_or(ApiError::NotFound)?;
    if let Some(ids) = &req.tag_ids {
        sqlx::query("DELETE FROM ip_tags WHERE ip_id = $1")
            .bind(id)
            .execute(&state.pool)
            .await?;
        for t in ids {
            sqlx::query(
                "INSERT INTO ip_tags (ip_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(t)
            .execute(&state.pool)
            .await?;
        }
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.ip.update",
        "ip_address",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(ip_dto(&state, row).await))
}

async fn delete_ip(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Operator)?;
    let result = sqlx::query("DELETE FROM ip_addresses WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.ip.delete",
        "ip_address",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn next_free_ip(
    State(state): State<AppState>,
    _c: Claims,
    Path(subnet_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let (cidr,): (IpNetwork,) = sqlx::query_as("SELECT cidr FROM subnets WHERE id = $1")
        .bind(subnet_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    let net = ipam_logic::parse_cidr(&cidr.to_string())?;
    let occupied: Vec<IpAddr> =
        sqlx::query_as::<_, (IpNetwork,)>("SELECT address FROM ip_addresses WHERE subnet_id = $1")
            .bind(subnet_id)
            .fetch_all(&state.pool)
            .await?
            .into_iter()
            .map(|(n,)| n.ip())
            .collect();
    match ipam_logic::next_free(&net, &occupied) {
        Some(ip) => Ok(Json(serde_json::json!({ "address": ip.to_string() }))),
        None => Err(ApiError::Conflict("sous-réseau plein".into())),
    }
}
