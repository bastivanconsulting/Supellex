use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use uuid::Uuid;
use validator::Validate;

use crate::{
    audit,
    auth::{Claims, Role, require_role},
    error::{ApiError, ApiResult},
    models::*,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update).delete(remove))
        .route("/{id}/assign", axum::routing::post(assign))
        .route("/{id}/assignments", get(assignments))
}

#[derive(sqlx::FromRow)]
struct AssetRow {
    id: Uuid,
    asset_tag: String,
    #[sqlx(rename = "type")]
    asset_type: String,
    status: String,
    manufacturer: Option<String>,
    model: Option<String>,
    serial_number: Option<String>,
    hostname: Option<String>,
    os: Option<String>,
    purchase_date: Option<chrono::NaiveDate>,
    warranty_end: Option<chrono::NaiveDate>,
    purchase_price: Option<String>,
    location_id: Option<Uuid>,
    assigned_to: Option<Uuid>,
    notes: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl AssetRow {
    fn into_dto(self) -> AssetDto {
        AssetDto {
            id: self.id,
            asset_tag: self.asset_tag,
            r#type: AssetType::from_db(&self.asset_type).unwrap_or(AssetType::Other),
            status: AssetStatus::from_db(&self.status).unwrap_or(AssetStatus::InStock),
            manufacturer: self.manufacturer,
            model: self.model,
            serial_number: self.serial_number,
            hostname: self.hostname,
            os: self.os,
            purchase_date: self.purchase_date,
            warranty_end: self.warranty_end,
            purchase_price: self.purchase_price.and_then(|v| v.parse::<f64>().ok()),
            location_id: self.location_id,
            assigned_to: self.assigned_to,
            notes: self.notes,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

// macro pour composer des requêtes statiques (concaténation au compile-time)
macro_rules! asset_query {
    ($tail:literal) => {
        concat!(
            "SELECT id, asset_tag, type::text AS type, status::text AS status, manufacturer, model,
             serial_number, hostname, os, purchase_date, warranty_end, purchase_price::text AS purchase_price,
             location_id, assigned_to, notes, created_at, updated_at FROM assets ", $tail
        )
    };
}

async fn list(
    State(state): State<AppState>,
    _claims: Claims,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Paged<AssetDto>>> {
    let filter = q.q.clone().unwrap_or_default();
    // Requête statique : filtre vide = match-all (pas de SQL dynamique).
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM assets
         WHERE ($1 = '' OR (asset_tag || ' ' || COALESCE(hostname,'') || ' ' || COALESCE(serial_number,'') || ' ' || COALESCE(model,'')) ILIKE '%' || $1 || '%')",
    )
    .bind(&filter)
    .fetch_one(&state.pool)
    .await?;
    let rows = sqlx::query_as::<_, AssetRow>(asset_query!(
        "WHERE ($3 = '' OR (asset_tag || ' ' || COALESCE(hostname,'') || ' ' || COALESCE(serial_number,'') || ' ' || COALESCE(model,'')) ILIKE '%' || $3 || '%')
         ORDER BY asset_tag LIMIT $1 OFFSET $2"
    ))
    .bind(q.limit())
    .bind(q.offset())
    .bind(&filter)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(Paged {
        items: rows.into_iter().map(AssetRow::into_dto).collect(),
        total: total.0,
        limit: q.limit(),
        offset: q.offset(),
    }))
}

async fn get_one(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AssetDto>> {
    let row = sqlx::query_as::<_, AssetRow>(asset_query!("WHERE id = $1"))
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(row.into_dto()))
}

async fn create(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertAssetRequest>,
) -> ApiResult<Json<AssetDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let status = req.status.unwrap_or(AssetStatus::InStock);
    let row = sqlx::query_as::<_, AssetRow>(
        "INSERT INTO assets (asset_tag, type, status, manufacturer, model, serial_number, hostname, os,
           purchase_date, warranty_end, purchase_price, location_id, assigned_to, notes)
         VALUES ($1, $2::asset_type, $3::asset_status, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         RETURNING id, asset_tag, type::text AS type, status::text AS status, manufacturer, model, serial_number,
           hostname, os, purchase_date, warranty_end, purchase_price::text AS purchase_price,
           location_id, assigned_to, notes, created_at, updated_at",
    )
    .bind(&req.asset_tag)
    .bind(req.r#type.as_str())
    .bind(status.as_str())
    .bind(&req.manufacturer)
    .bind(&req.model)
    .bind(&req.serial_number)
    .bind(&req.hostname)
    .bind(&req.os)
    .bind(req.purchase_date)
    .bind(req.warranty_end)
    .bind(req.purchase_price)
    .bind(req.location_id)
    .bind(req.assigned_to)
    .bind(&req.notes)
    .fetch_optional(&state.pool).await?
    .ok_or(ApiError::Conflict("existe déjà".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "asset.create",
        "asset",
        Some(row.id.to_string()),
    )
    .details(serde_json::json!({"asset_tag": req.asset_tag, "type": req.r#type}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(row.into_dto()))
}

async fn update(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertAssetRequest>,
) -> ApiResult<Json<AssetDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let status = req.status.unwrap_or(AssetStatus::InStock);
    let row = sqlx::query_as::<_, AssetRow>(
        "UPDATE assets SET asset_tag=$2, type=$3::asset_type, status=$4::asset_status, manufacturer=$5, model=$6,
           serial_number=$7, hostname=$8, os=$9, purchase_date=$10, warranty_end=$11, purchase_price=$12,
           location_id=$13, assigned_to=$14, notes=$15
         WHERE id=$1
         RETURNING id, asset_tag, type::text AS type, status::text AS status, manufacturer, model, serial_number,
           hostname, os, purchase_date, warranty_end, purchase_price::text AS purchase_price,
           location_id, assigned_to, notes, created_at, updated_at",
    )
    .bind(id)
    .bind(&req.asset_tag)
    .bind(req.r#type.as_str())
    .bind(status.as_str())
    .bind(&req.manufacturer)
    .bind(&req.model)
    .bind(&req.serial_number)
    .bind(&req.hostname)
    .bind(&req.os)
    .bind(req.purchase_date)
    .bind(req.warranty_end)
    .bind(req.purchase_price)
    .bind(req.location_id)
    .bind(req.assigned_to)
    .bind(&req.notes)
    .fetch_optional(&state.pool).await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(Some(&claims), "asset.update", "asset", Some(id.to_string()))
        .details(serde_json::json!({"asset_tag": req.asset_tag}))
        .ip(None)
        .write(&state.pool)
        .await;
    Ok(Json(row.into_dto()))
}

async fn remove(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM assets WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(Some(&claims), "asset.delete", "asset", Some(id.to_string()))
        .details(serde_json::json!({}))
        .ip(None)
        .write(&state.pool)
        .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Assigne un actif à une personne (ou le rend : person_id = null).
/// Met à jour assets.assigned_to ET l'historique asset_assignments.
async fn assign(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<AssignAssetRequest>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let mut tx = state.pool.begin().await?;
    // Clôt l'assignation en cours
    sqlx::query("UPDATE asset_assignments SET returned_at = now() WHERE asset_id = $1 AND returned_at IS NULL")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO asset_assignments (asset_id, person_id, note) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(req.person_id)
        .bind(&req.note)
        .execute(&mut *tx)
        .await?;
    let new_status = if req.person_id.is_some() {
        AssetStatus::Deployed
    } else {
        AssetStatus::InStock
    };
    let result =
        sqlx::query("UPDATE assets SET assigned_to = $2, status = $3::asset_status WHERE id = $1")
            .bind(id)
            .bind(req.person_id)
            .bind(new_status.as_str())
            .execute(&mut *tx)
            .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    tx.commit().await?;
    audit::AuditEvent::new(Some(&claims), "asset.assign", "asset", Some(id.to_string()))
        .details(serde_json::json!({"person_id": req.person_id}))
        .ip(None)
        .write(&state.pool)
        .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn assignments(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<AssetAssignmentDto>>> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid, Option<Uuid>, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>, Option<String>)>(
        "SELECT id, asset_id, person_id, assigned_at, returned_at, note FROM asset_assignments WHERE asset_id = $1 ORDER BY assigned_at DESC",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(id, asset_id, person_id, assigned_at, returned_at, note)| AssetAssignmentDto {
                    id,
                    asset_id,
                    person_id,
                    assigned_at,
                    returned_at,
                    note,
                },
            )
            .collect(),
    ))
}
