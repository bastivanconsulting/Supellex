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
}

async fn list(
    State(state): State<AppState>,
    _claims: Claims,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Paged<PersonDto>>> {
    let filter = q.q.clone().unwrap_or_default();
    // Requêtes statiques : filtre vide = match-all.
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM people
         WHERE ($1 = '' OR (first_name || ' ' || last_name || ' ' || COALESCE(email::text,'')) ILIKE '%' || $1 || '%')",
    )
    .bind(&filter)
    .fetch_one(&state.pool)
    .await?;
    let rows = sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<String>, Option<String>, Option<String>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, first_name, last_name, email::text, phone, department, job_title, created_at
         FROM people
         WHERE ($3 = '' OR (first_name || ' ' || last_name || ' ' || COALESCE(email::text,'')) ILIKE '%' || $3 || '%')
         ORDER BY last_name, first_name LIMIT $1 OFFSET $2",
    )
    .bind(q.limit())
    .bind(q.offset())
    .bind(&filter)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(Paged {
        items: rows
            .into_iter()
            .map(
                |(id, first_name, last_name, email, phone, department, job_title, created_at)| {
                    PersonDto {
                        id,
                        first_name,
                        last_name,
                        email,
                        phone,
                        department,
                        job_title,
                        created_at,
                    }
                },
            )
            .collect(),
        total: total.0,
        limit: q.limit(),
        offset: q.offset(),
    }))
}

async fn get_one(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PersonDto>> {
    let row = sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<String>, Option<String>, Option<String>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, first_name, last_name, email::text, phone, department, job_title, created_at FROM people WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(PersonDto {
        id: row.0,
        first_name: row.1,
        last_name: row.2,
        email: row.3,
        phone: row.4,
        department: row.5,
        job_title: row.6,
        created_at: row.7,
    }))
}

async fn create(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertPersonRequest>,
) -> ApiResult<Json<PersonDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let row = sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<String>, Option<String>, Option<String>, chrono::DateTime<chrono::Utc>)>(
        "INSERT INTO people (first_name, last_name, email, phone, department, job_title)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, first_name, last_name, email::text, phone, department, job_title, created_at",
    )
    .bind(&req.first_name)
    .bind(&req.last_name)
    .bind(req.email.as_deref().map(|e| e.to_lowercase()))
    .bind(&req.phone)
    .bind(&req.department)
    .bind(&req.job_title)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("existe déjà".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "person.create",
        "person",
        Some(row.0.to_string()),
    )
    .details(serde_json::json!({"name": format!("{} {}", req.first_name, req.last_name)}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(PersonDto {
        id: row.0,
        first_name: row.1,
        last_name: row.2,
        email: row.3,
        phone: row.4,
        department: row.5,
        job_title: row.6,
        created_at: row.7,
    }))
}

async fn update(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertPersonRequest>,
) -> ApiResult<Json<PersonDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let row = sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<String>, Option<String>, Option<String>, chrono::DateTime<chrono::Utc>)>(
        "UPDATE people SET first_name=$2, last_name=$3, email=$4, phone=$5, department=$6, job_title=$7 WHERE id=$1
         RETURNING id, first_name, last_name, email::text, phone, department, job_title, created_at",
    )
    .bind(id)
    .bind(&req.first_name)
    .bind(&req.last_name)
    .bind(req.email.as_deref().map(|e| e.to_lowercase()))
    .bind(&req.phone)
    .bind(&req.department)
    .bind(&req.job_title)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "person.update",
        "person",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(PersonDto {
        id: row.0,
        first_name: row.1,
        last_name: row.2,
        email: row.3,
        phone: row.4,
        department: row.5,
        job_title: row.6,
        created_at: row.7,
    }))
}

async fn remove(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Operator)?;
    let result = sqlx::query("DELETE FROM people WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "person.delete",
        "person",
        Some(id.to_string()),
    )
    .details(serde_json::json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
