use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};

use crate::{
    auth::{Claims, Role, require_role},
    error::ApiResult,
    models::*,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_audit))
}

/// Journal d'audit : réservé au rôle admin (données de traçabilité sensibles).
async fn list_audit(
    State(state): State<AppState>,
    claims: Claims,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Paged<AuditEntryDto>>> {
    require_role(&claims, Role::Admin)?;
    let filter = q.q.clone().unwrap_or_default();
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_log
         WHERE ($1 = '' OR (action || ' ' || entity_type || ' ' || COALESCE(actor_email,'')) ILIKE '%' || $1 || '%')",
    )
    .bind(&filter)
    .fetch_one(&state.pool).await?;
    let rows = sqlx::query_as::<_, (i64, chrono::DateTime<chrono::Utc>, Option<String>, String, String, Option<String>, serde_json::Value, Option<ipnetwork::IpNetwork>)>(
        "SELECT id, at, actor_email, action, entity_type, entity_id, details, ip
         FROM audit_log
         WHERE ($3 = '' OR (action || ' ' || entity_type || ' ' || COALESCE(actor_email,'')) ILIKE '%' || $3 || '%')
         ORDER BY at DESC LIMIT $1 OFFSET $2",
    )
    .bind(q.limit()).bind(q.offset()).bind(&filter)
    .fetch_all(&state.pool).await?;
    Ok(Json(Paged {
        items: rows
            .into_iter()
            .map(
                |(id, at, actor_email, action, entity_type, entity_id, details, ip)| {
                    AuditEntryDto {
                        id,
                        at,
                        actor_email,
                        action,
                        entity_type,
                        entity_id,
                        details,
                        ip: ip.map(|i| i.ip().to_string()),
                    }
                },
            )
            .collect(),
        total: total.0,
        limit: q.limit(),
        offset: q.offset(),
    }))
}
