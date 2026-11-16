use axum::{
    Json, Router,
    extract::{Path, State},
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
}

type LocationRow = (
    Uuid,
    String,
    String,
    Option<Uuid>,
    Option<String>,
    Option<String>,
);

fn into_dto(r: LocationRow) -> LocationDto {
    LocationDto {
        id: r.0,
        name: r.1,
        kind: r.2,
        parent_id: r.3,
        address: r.4,
        description: r.5,
    }
}

async fn list(State(state): State<AppState>, _claims: Claims) -> ApiResult<Json<Vec<LocationDto>>> {
    let rows = sqlx::query_as::<_, LocationRow>(
        "SELECT id, name, kind, parent_id, address, description FROM locations ORDER BY kind, name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(into_dto).collect()))
}

async fn get_one(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LocationDto>> {
    let row = sqlx::query_as::<_, LocationRow>(
        "SELECT id, name, kind, parent_id, address, description FROM locations WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(into_dto(row)))
}

async fn create(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertLocationRequest>,
) -> ApiResult<Json<LocationDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if let Some(p) = req.parent_id {
        let parent: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM locations WHERE id = $1")
            .bind(p)
            .fetch_optional(&state.pool)
            .await?;
        if parent.is_none() {
            return Err(ApiError::Validation(
                "emplacement parent introuvable".into(),
            ));
        }
    }
    let row = sqlx::query_as::<_, LocationRow>(
        "INSERT INTO locations (name, kind, parent_id, address, description)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, kind, parent_id, address, description",
    )
    .bind(&req.name)
    .bind(&req.kind)
    .bind(req.parent_id)
    .bind(&req.address)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("existe déjà".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "location.create",
        "location",
        Some(row.0.to_string()),
    )
    .details(serde_json::json!({"name": req.name, "kind": req.kind}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(into_dto(row)))
}

async fn update(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertLocationRequest>,
) -> ApiResult<Json<LocationDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if req.parent_id == Some(id) {
        return Err(ApiError::Validation(
            "un emplacement ne peut pas être son propre parent".into(),
        ));
    }
    let row = sqlx::query_as::<_, LocationRow>(
        "UPDATE locations SET name=$2, kind=$3, parent_id=$4, address=$5, description=$6 WHERE id=$1
         RETURNING id, name, kind, parent_id, address, description",
    )
    .bind(id)
    .bind(&req.name)
    .bind(&req.kind)
    .bind(req.parent_id)
    .bind(&req.address)
    .bind(&req.description)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "location.update",
        "location",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(into_dto(row)))
}

async fn remove(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM locations WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "location.delete",
        "location",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
