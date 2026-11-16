//! Catalogue PHPIPAM : domaines VLAN, tags, plages DHCP, règles NAT,
//! définitions de champs personnalisés et recherche globale.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use ipnetwork::IpNetwork;
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::IpAddr;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

use crate::{
    audit,
    auth::{Claims, Role, require_role},
    error::{ApiError, ApiResult},
    ipam_logic,
    models::*,
    state::AppState,
};

pub fn domains_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_domains).post(create_domain))
        .route(
            "/{id}",
            get(get_domain).put(update_domain).delete(delete_domain),
        )
}

pub fn tags_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tags).post(create_tag))
        .route("/{id}", get(get_tag).put(update_tag).delete(delete_tag))
}

pub fn dhcp_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_dhcp_ranges).post(create_dhcp_range))
        .route(
            "/{id}",
            get(get_dhcp_range)
                .put(update_dhcp_range)
                .delete(delete_dhcp_range),
        )
}

pub fn nat_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_nat).post(create_nat))
        .route("/{id}", get(get_nat).put(update_nat).delete(delete_nat))
}

pub fn fields_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_fields).post(create_field))
        .route(
            "/{id}",
            get(get_field).put(update_field).delete(delete_field),
        )
}

pub fn search_router() -> Router<AppState> {
    Router::new().route("/", get(search))
}

type DomainRow = (Uuid, String, Option<String>);
fn domain_dto(r: DomainRow) -> VlanDomainDto {
    VlanDomainDto {
        id: r.0,
        name: r.1,
        description: r.2,
    }
}

async fn list_domains(
    State(state): State<AppState>,
    _c: Claims,
) -> ApiResult<Json<Vec<VlanDomainDto>>> {
    let rows = sqlx::query_as::<_, DomainRow>(
        "SELECT id, name, description FROM vlan_domains ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(domain_dto).collect()))
}

async fn get_domain(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<VlanDomainDto>> {
    let row = sqlx::query_as::<_, DomainRow>(
        "SELECT id, name, description FROM vlan_domains WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(domain_dto(row)))
}

async fn create_domain(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertVlanDomainRequest>,
) -> ApiResult<Json<VlanDomainDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let row = sqlx::query_as::<_, DomainRow>(
        "INSERT INTO vlan_domains (name, description) VALUES ($1, $2) RETURNING id, name, description",
    )
    .bind(&req.name)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("domaine existant".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.vlandomain.create",
        "vlan_domain",
        Some(row.0.to_string()),
    )
    .details(json!({"name": req.name}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(domain_dto(row)))
}

async fn update_domain(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertVlanDomainRequest>,
) -> ApiResult<Json<VlanDomainDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let row = sqlx::query_as::<_, DomainRow>(
        "UPDATE vlan_domains SET name=$2, description=$3 WHERE id=$1 RETURNING id, name, description",
    )
    .bind(id)
    .bind(&req.name)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.vlandomain.update",
        "vlan_domain",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(domain_dto(row)))
}

async fn delete_domain(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM vlan_domains WHERE id = $1 AND name <> 'Default'")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.vlandomain.delete",
        "vlan_domain",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Domaine VLAN par défaut (créé par la migration).
pub async fn default_domain_id(state: &AppState) -> Result<Uuid, ApiError> {
    let (id,): (Uuid,) = sqlx::query_as("SELECT id FROM vlan_domains WHERE name = 'Default'")
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::Internal("domaine VLAN Default absent".into()))?;
    Ok(id)
}

// ---------------- Tags ----------------

type TagRow = (Uuid, String, String);
fn tag_dto(r: TagRow) -> TagDto {
    TagDto {
        id: r.0,
        name: r.1,
        color: r.2,
    }
}

async fn list_tags(State(state): State<AppState>, _c: Claims) -> ApiResult<Json<Vec<TagDto>>> {
    let rows = sqlx::query_as::<_, TagRow>("SELECT id, name, color FROM tags ORDER BY name")
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows.into_iter().map(tag_dto).collect()))
}

async fn get_tag(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TagDto>> {
    let row = sqlx::query_as::<_, TagRow>("SELECT id, name, color FROM tags WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(tag_dto(row)))
}

fn normalize_color(c: &Option<String>) -> Result<String, ApiError> {
    match c {
        Some(c) if is_valid_color(c) => Ok(c.to_ascii_uppercase()),
        Some(_) => Err(ApiError::Validation(
            "couleur invalide (format #RRGGBB)".into(),
        )),
        None => Ok("#0E7490".into()),
    }
}

async fn create_tag(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertTagRequest>,
) -> ApiResult<Json<TagDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let color = normalize_color(&req.color)?;
    let row = sqlx::query_as::<_, TagRow>(
        "INSERT INTO tags (name, color) VALUES ($1, $2) RETURNING id, name, color",
    )
    .bind(&req.name)
    .bind(&color)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("tag existant".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.tag.create",
        "tag",
        Some(row.0.to_string()),
    )
    .details(json!({"name": req.name}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(tag_dto(row)))
}

async fn update_tag(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertTagRequest>,
) -> ApiResult<Json<TagDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let color = normalize_color(&req.color)?;
    let row = sqlx::query_as::<_, TagRow>(
        "UPDATE tags SET name=$2, color=$3 WHERE id=$1 RETURNING id, name, color",
    )
    .bind(id)
    .bind(&req.name)
    .bind(&color)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.tag.update",
        "tag",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(tag_dto(row)))
}

async fn delete_tag(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM tags WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.tag.delete",
        "tag",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Remplace les tags d'une IP (PUT /ipam/ips/{id}/tags).
pub async fn set_ip_tags(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<Json<Vec<TagDto>>> {
    require_role(&claims, Role::Operator)?;
    let tag_ids: Vec<Uuid> =
        serde_json::from_value(body.get("tag_ids").cloned().unwrap_or(Value::Null))
            .map_err(|_| ApiError::Validation("tag_ids attendu".into()))?;
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM ip_addresses WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }
    sqlx::query("DELETE FROM ip_tags WHERE ip_id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    for t in &tag_ids {
        sqlx::query("INSERT INTO ip_tags (ip_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(t)
            .execute(&state.pool)
            .await?;
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.ip.tags",
        "ip_address",
        Some(id.to_string()),
    )
    .details(json!({"tag_ids": tag_ids}))
    .ip(None)
    .write(&state.pool)
    .await;
    let rows = sqlx::query_as::<_, TagRow>(
        "SELECT t.id, t.name, t.color FROM tags t JOIN ip_tags it ON it.tag_id = t.id WHERE it.ip_id = $1 ORDER BY t.name",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(tag_dto).collect()))
}

/// Tags d'une IP (requête interne réutilisée par les DTO d'IP).
pub async fn tags_of_ip(pool: &sqlx::PgPool, ip_id: Uuid) -> Vec<TagDto> {
    sqlx::query_as::<_, TagRow>(
        "SELECT t.id, t.name, t.color FROM tags t JOIN ip_tags it ON it.tag_id = t.id WHERE it.ip_id = $1 ORDER BY t.name",
    )
    .bind(ip_id)
    .fetch_all(pool)
    .await
    .map(|rows| rows.into_iter().map(tag_dto).collect())
    .unwrap_or_default()
}

// ---------------- Plages DHCP ----------------

type DhcpRow = (
    Uuid,
    Uuid,
    IpNetwork,
    IpNetwork,
    Option<String>,
    Option<String>,
);
fn dhcp_dto(r: DhcpRow) -> DhcpRangeDto {
    DhcpRangeDto {
        id: r.0,
        subnet_id: r.1,
        start_addr: r.2.ip().to_string(),
        end_addr: r.3.ip().to_string(),
        name: r.4,
        description: r.5,
    }
}

#[derive(Debug, Deserialize)]
pub struct DhcpListQuery {
    pub subnet_id: Option<Uuid>,
}

async fn list_dhcp_ranges(
    State(state): State<AppState>,
    _c: Claims,
    Query(q): Query<DhcpListQuery>,
) -> ApiResult<Json<Vec<DhcpRangeDto>>> {
    let rows = match q.subnet_id {
        Some(sid) => sqlx::query_as::<_, DhcpRow>(
            "SELECT id, subnet_id, start_addr, end_addr, name, description FROM dhcp_ranges WHERE subnet_id = $1 ORDER BY start_addr",
        )
        .bind(sid)
        .fetch_all(&state.pool)
        .await?,
        None => sqlx::query_as::<_, DhcpRow>(
            "SELECT id, subnet_id, start_addr, end_addr, name, description FROM dhcp_ranges ORDER BY start_addr",
        )
        .fetch_all(&state.pool)
        .await?,
    };
    Ok(Json(rows.into_iter().map(dhcp_dto).collect()))
}

async fn get_dhcp_range(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<DhcpRangeDto>> {
    let row = sqlx::query_as::<_, DhcpRow>(
        "SELECT id, subnet_id, start_addr, end_addr, name, description FROM dhcp_ranges WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(dhcp_dto(row)))
}

/// Valide qu'une plage est dans le sous-réseau et ne chevauche pas les autres.
async fn validate_range(
    state: &AppState,
    subnet_id: Uuid,
    start: IpAddr,
    end: IpAddr,
    ignore_id: Option<Uuid>,
) -> Result<(), ApiError> {
    if start > end {
        return Err(ApiError::Validation(
            "début de plage supérieur à la fin".into(),
        ));
    }
    let (cidr,): (IpNetwork,) = sqlx::query_as("SELECT cidr FROM subnets WHERE id = $1")
        .bind(subnet_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    let net = ipam_logic::parse_cidr(&cidr.to_string())?;
    if !net.contains(start) || !net.contains(end) {
        return Err(ApiError::Validation(
            "la plage doit être incluse dans le sous-réseau".into(),
        ));
    }
    let existing = sqlx::query_as::<_, (Uuid, IpNetwork, IpNetwork)>(
        "SELECT id, start_addr, end_addr FROM dhcp_ranges WHERE subnet_id = $1",
    )
    .bind(subnet_id)
    .fetch_all(&state.pool)
    .await?;
    for (id, s, e) in existing {
        if ignore_id == Some(id) {
            continue;
        }
        // Chevauchement si les deux plages sont de la même famille
        if std::mem::discriminant(&start) == std::mem::discriminant(&s.ip())
            && !(end < s.ip() || start > e.ip())
        {
            return Err(ApiError::Conflict(format!(
                "chevauche la plage existante {s} - {e}"
            )));
        }
    }
    Ok(())
}

async fn create_dhcp_range(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertDhcpRangeRequest>,
) -> ApiResult<Json<DhcpRangeDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let start = IpAddr::from_str(&req.start_addr)
        .map_err(|_| ApiError::Validation("start_addr invalide".into()))?;
    let end = IpAddr::from_str(&req.end_addr)
        .map_err(|_| ApiError::Validation("end_addr invalide".into()))?;
    validate_range(&state, req.subnet_id, start, end, None).await?;
    let row = sqlx::query_as::<_, DhcpRow>(
        "INSERT INTO dhcp_ranges (subnet_id, start_addr, end_addr, name, description)
         VALUES ($1, $2::inet, $3::inet, $4, $5)
         RETURNING id, subnet_id, start_addr, end_addr, name, description",
    )
    .bind(req.subnet_id)
    .bind(IpNetwork::from(start))
    .bind(IpNetwork::from(end))
    .bind(&req.name)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("plage existante".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.dhcp.create",
        "dhcp_range",
        Some(row.0.to_string()),
    )
    .details(json!({"subnet_id": req.subnet_id, "start": req.start_addr, "end": req.end_addr}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(dhcp_dto(row)))
}

async fn update_dhcp_range(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertDhcpRangeRequest>,
) -> ApiResult<Json<DhcpRangeDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let start = IpAddr::from_str(&req.start_addr)
        .map_err(|_| ApiError::Validation("start_addr invalide".into()))?;
    let end = IpAddr::from_str(&req.end_addr)
        .map_err(|_| ApiError::Validation("end_addr invalide".into()))?;
    validate_range(&state, req.subnet_id, start, end, Some(id)).await?;
    let row = sqlx::query_as::<_, DhcpRow>(
        "UPDATE dhcp_ranges SET subnet_id=$2, start_addr=$3::inet, end_addr=$4::inet, name=$5, description=$6
         WHERE id=$1
         RETURNING id, subnet_id, start_addr, end_addr, name, description",
    )
    .bind(id)
    .bind(req.subnet_id)
    .bind(IpNetwork::from(start))
    .bind(IpNetwork::from(end))
    .bind(&req.name)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.dhcp.update",
        "dhcp_range",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(dhcp_dto(row)))
}

async fn delete_dhcp_range(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM dhcp_ranges WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.dhcp.delete",
        "dhcp_range",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------- NAT ----------------

type NatRow = (
    Uuid,
    String,
    Option<Uuid>,
    Option<IpNetwork>,
    Option<IpNetwork>,
    Option<String>,
);
fn nat_dto(r: NatRow) -> NatRuleDto {
    NatRuleDto {
        id: r.0,
        name: r.1,
        src_subnet_id: r.2,
        src_address: r.3.map(|n| n.ip().to_string()),
        dst_address: r.4.map(|n| n.ip().to_string()),
        description: r.5,
    }
}

async fn list_nat(State(state): State<AppState>, _c: Claims) -> ApiResult<Json<Vec<NatRuleDto>>> {
    let rows = sqlx::query_as::<_, NatRow>(
        "SELECT id, name, src_subnet_id, src_address, dst_address, description FROM nat_rules ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(nat_dto).collect()))
}

async fn get_nat(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<NatRuleDto>> {
    let row = sqlx::query_as::<_, NatRow>(
        "SELECT id, name, src_subnet_id, src_address, dst_address, description FROM nat_rules WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(nat_dto(row)))
}

fn parse_opt_ip(s: &Option<String>, field: &str) -> Result<Option<IpAddr>, ApiError> {
    s.as_deref()
        .map(|v| IpAddr::from_str(v).map_err(|_| ApiError::Validation(format!("{field} invalide"))))
        .transpose()
}

async fn create_nat(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertNatRuleRequest>,
) -> ApiResult<Json<NatRuleDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if req.src_address.is_none() && req.src_subnet_id.is_none() {
        return Err(ApiError::Validation(
            "src_address ou src_subnet_id requis".into(),
        ));
    }
    let src = parse_opt_ip(&req.src_address, "src_address")?;
    let dst = parse_opt_ip(&req.dst_address, "dst_address")?;
    let row = sqlx::query_as::<_, NatRow>(
        "INSERT INTO nat_rules (name, src_subnet_id, src_address, dst_address, description)
         VALUES ($1, $2, $3::inet, $4::inet, $5)
         RETURNING id, name, src_subnet_id, src_address, dst_address, description",
    )
    .bind(&req.name)
    .bind(req.src_subnet_id)
    .bind(src.map(IpNetwork::from))
    .bind(dst.map(IpNetwork::from))
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("règle existante".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.nat.create",
        "nat_rule",
        Some(row.0.to_string()),
    )
    .details(json!({"name": req.name}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(nat_dto(row)))
}

async fn update_nat(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertNatRuleRequest>,
) -> ApiResult<Json<NatRuleDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if req.src_address.is_none() && req.src_subnet_id.is_none() {
        return Err(ApiError::Validation(
            "src_address ou src_subnet_id requis".into(),
        ));
    }
    let src = parse_opt_ip(&req.src_address, "src_address")?;
    let dst = parse_opt_ip(&req.dst_address, "dst_address")?;
    let row = sqlx::query_as::<_, NatRow>(
        "UPDATE nat_rules SET name=$2, src_subnet_id=$3, src_address=$4::inet, dst_address=$5::inet, description=$6
         WHERE id=$1
         RETURNING id, name, src_subnet_id, src_address, dst_address, description",
    )
    .bind(id)
    .bind(&req.name)
    .bind(req.src_subnet_id)
    .bind(src.map(IpNetwork::from))
    .bind(dst.map(IpNetwork::from))
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.nat.update",
        "nat_rule",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(nat_dto(row)))
}

async fn delete_nat(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM nat_rules WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.nat.delete",
        "nat_rule",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------- Champs personnalisés ----------------

type FieldRow = (Uuid, String, String, String, Value, bool);
fn field_dto(r: FieldRow) -> FieldDefDto {
    FieldDefDto {
        id: r.0,
        entity: r.1,
        name: r.2,
        field_type: r.3,
        options: serde_json::from_value(r.4).unwrap_or_default(),
        required: r.5,
    }
}

#[derive(Debug, Deserialize)]
pub struct FieldListQuery {
    pub entity: Option<String>,
}

async fn list_fields(
    State(state): State<AppState>,
    _c: Claims,
    Query(q): Query<FieldListQuery>,
) -> ApiResult<Json<Vec<FieldDefDto>>> {
    let rows = match q.entity {
        Some(e) => sqlx::query_as::<_, FieldRow>(
            "SELECT id, entity, name, field_type, options, required FROM field_defs WHERE entity = $1 ORDER BY name",
        )
        .bind(&e)
        .fetch_all(&state.pool)
        .await?,
        None => sqlx::query_as::<_, FieldRow>(
            "SELECT id, entity, name, field_type, options, required FROM field_defs ORDER BY entity, name",
        )
        .fetch_all(&state.pool)
        .await?,
    };
    Ok(Json(rows.into_iter().map(field_dto).collect()))
}

async fn get_field(
    State(state): State<AppState>,
    _c: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<FieldDefDto>> {
    let row = sqlx::query_as::<_, FieldRow>(
        "SELECT id, entity, name, field_type, options, required FROM field_defs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(field_dto(row)))
}

async fn create_field(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertFieldDefRequest>,
) -> ApiResult<Json<FieldDefDto>> {
    require_role(&claims, Role::Admin)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if req.field_type == "select" && req.options.as_ref().is_none_or(|o| o.is_empty()) {
        return Err(ApiError::Validation(
            "options requises pour un champ select".into(),
        ));
    }
    let options = json!(req.options.unwrap_or_default());
    let row = sqlx::query_as::<_, FieldRow>(
        "INSERT INTO field_defs (entity, name, field_type, options, required)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, entity, name, field_type, options, required",
    )
    .bind(&req.entity)
    .bind(&req.name)
    .bind(&req.field_type)
    .bind(&options)
    .bind(req.required.unwrap_or(false))
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("champ existant".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.field.create",
        "field_def",
        Some(row.0.to_string()),
    )
    .details(json!({"entity": req.entity, "name": req.name}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(field_dto(row)))
}

async fn update_field(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertFieldDefRequest>,
) -> ApiResult<Json<FieldDefDto>> {
    require_role(&claims, Role::Admin)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if req.field_type == "select" && req.options.as_ref().is_none_or(|o| o.is_empty()) {
        return Err(ApiError::Validation(
            "options requises pour un champ select".into(),
        ));
    }
    let options = json!(req.options.unwrap_or_default());
    let row = sqlx::query_as::<_, FieldRow>(
        "UPDATE field_defs SET entity=$2, name=$3, field_type=$4, options=$5, required=$6
         WHERE id=$1
         RETURNING id, entity, name, field_type, options, required",
    )
    .bind(id)
    .bind(&req.entity)
    .bind(&req.name)
    .bind(&req.field_type)
    .bind(&options)
    .bind(req.required.unwrap_or(false))
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.field.update",
        "field_def",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(field_dto(row)))
}

async fn delete_field(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM field_defs WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "ipam.field.delete",
        "field_def",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Valide un objet custom_fields contre les définitions field_defs de l'entité :
/// clés inconnues refusées, champs requis présents, type respecté.
pub async fn validate_custom_fields(
    pool: &sqlx::PgPool,
    entity: &str,
    value: &Value,
) -> Result<(), ApiError> {
    let Some(obj) = value.as_object() else {
        return Err(ApiError::Validation(
            "custom_fields doit être un objet".into(),
        ));
    };
    let defs = sqlx::query_as::<_, (String, String, Value, bool)>(
        "SELECT name, field_type, options, required FROM field_defs WHERE entity = $1",
    )
    .bind(entity)
    .fetch_all(pool)
    .await?;
    for (name, ftype, options, required) in &defs {
        match obj.get(name) {
            None => {
                if *required {
                    return Err(ApiError::Validation(format!(
                        "champ requis manquant : {name}"
                    )));
                }
            }
            Some(v) => {
                let ok = match ftype.as_str() {
                    "text" => v.is_string(),
                    "number" => v.is_number(),
                    "date" => v
                        .as_str()
                        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                        .is_some(),
                    "select" => v
                        .as_str()
                        .map(|s| {
                            serde_json::from_value::<Vec<String>>(options.clone())
                                .map(|opts| opts.contains(&s.to_string()))
                                .unwrap_or(false)
                        })
                        .unwrap_or(false),
                    _ => false,
                };
                if !ok {
                    return Err(ApiError::Validation(format!(
                        "valeur invalide pour le champ {name} (type {ftype})"
                    )));
                }
            }
        }
    }
    for key in obj.keys() {
        if !defs.iter().any(|(n, _, _, _)| n == key) {
            return Err(ApiError::Validation(format!(
                "champ personnalisé inconnu : {key}"
            )));
        }
    }
    Ok(())
}

// ---------------- Recherche globale ----------------

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<i64>,
}

async fn search(
    State(state): State<AppState>,
    _c: Claims,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Vec<SearchHit>>> {
    let pat = format!("%{}%", q.q);
    let lim = q.limit.unwrap_or(10).clamp(1, 50);
    let mut hits = Vec::new();

    let ips = sqlx::query_as::<
        _,
        (
            Uuid,
            IpNetwork,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
        ),
    >(
        "SELECT id, address, hostname, mac, status::text, description FROM ip_addresses
         WHERE address::text ILIKE $1 OR COALESCE(hostname,'') ILIKE $1 OR COALESCE(mac,'') ILIKE $1
            OR COALESCE(description,'') ILIKE $1
         ORDER BY address LIMIT $2",
    )
    .bind(&pat)
    .bind(lim)
    .fetch_all(&state.pool)
    .await?;
    for (id, addr, host, mac, status, desc) in ips {
        hits.push(SearchHit {
            kind: "ip",
            id: id.to_string(),
            label: addr.ip().to_string(),
            detail: format!(
                "{status} — {} {} {}",
                host.unwrap_or_default(),
                mac.map(|m| format!("MAC {m}")).unwrap_or_default(),
                desc.unwrap_or_default()
            ),
        });
    }

    let subnets = sqlx::query_as::<_, (Uuid, IpNetwork, Option<String>)>(
        "SELECT id, cidr, name FROM subnets WHERE cidr::text ILIKE $1 OR COALESCE(name,'') ILIKE $1 LIMIT $2",
    )
    .bind(&pat)
    .bind(lim)
    .fetch_all(&state.pool)
    .await?;
    for (id, cidr, name) in subnets {
        hits.push(SearchHit {
            kind: "subnet",
            id: id.to_string(),
            label: cidr.to_string(),
            detail: name.unwrap_or_default(),
        });
    }

    let assets = sqlx::query_as::<_, (Uuid, String, Option<String>, Option<String>)>(
        "SELECT id, asset_tag, hostname, serial_number FROM assets
         WHERE asset_tag ILIKE $1 OR COALESCE(hostname,'') ILIKE $1 OR COALESCE(serial_number,'') ILIKE $1
            OR COALESCE(model,'') ILIKE $1 OR COALESCE(manufacturer,'') ILIKE $1 LIMIT $2",
    )
    .bind(&pat)
    .bind(lim)
    .fetch_all(&state.pool)
    .await?;
    for (id, tag, host, serial) in assets {
        hits.push(SearchHit {
            kind: "asset",
            id: id.to_string(),
            label: tag,
            detail: format!(
                "{} {}",
                host.unwrap_or_default(),
                serial.map(|s| format!("n° {s}")).unwrap_or_default()
            ),
        });
    }

    let people = sqlx::query_as::<_, (Uuid, String, String, Option<String>)>(
        "SELECT id, first_name, last_name, email FROM people
         WHERE (first_name || ' ' || last_name) ILIKE $1 OR COALESCE(email,'') ILIKE $1 LIMIT $2",
    )
    .bind(&pat)
    .bind(lim)
    .fetch_all(&state.pool)
    .await?;
    for (id, first, last, email) in people {
        hits.push(SearchHit {
            kind: "person",
            id: id.to_string(),
            label: format!("{first} {last}"),
            detail: email.unwrap_or_default(),
        });
    }

    let vlans = sqlx::query_as::<_, (Uuid, i32, String)>(
        "SELECT id, vid, name FROM vlans WHERE name ILIKE $1 OR vid::text = $3 LIMIT $2",
    )
    .bind(&pat)
    .bind(lim)
    .bind(&q.q)
    .fetch_all(&state.pool)
    .await?;
    for (id, vid, name) in vlans {
        hits.push(SearchHit {
            kind: "vlan",
            id: id.to_string(),
            label: format!("VLAN {vid}"),
            detail: name,
        });
    }

    Ok(Json(hits))
}
