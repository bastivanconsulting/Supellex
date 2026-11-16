//! Administration des sources d'authentification externes (LDAP / OIDC) :
//! CRUD réservé admin, secrets chiffrés au repos et masqués en lecture,
//! bouton « Tester » (bind service LDAP / discovery OIDC). Toute mutation
//! est tracée dans l'audit log.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use crate::{
    audit,
    auth::{Claims, Role, require_role},
    auth_providers::{self, LdapConfig, OidcConfig, ProviderKind},
    error::{ApiError, ApiResult},
    ldap_auth,
    models::*,
    oidc_auth,
    state::AppState,
};

type ProviderRow = (
    Uuid,
    String,
    String,
    String,
    bool,
    Value,
    Option<DateTime<Utc>>,
    Option<bool>,
    DateTime<Utc>,
);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::put(update).delete(remove))
        .route("/{id}/test", axum::routing::post(test))
}

fn to_dto(r: ProviderRow) -> AuthProviderDto {
    let kind = ProviderKind::from_db(&r.3).unwrap_or(ProviderKind::Ldap);
    AuthProviderDto {
        id: r.0,
        slug: r.1,
        label: r.2,
        kind: r.3,
        enabled: r.4,
        config: auth_providers::mask_secrets(kind, &r.5),
        last_test_at: r.6,
        last_test_ok: r.7,
        created_at: r.8,
    }
}

/// Colonnes communes (chaîne constante, jamais injectée : sqlx 0.9 exige des
/// littéraux statiques).
macro_rules! select_cols {
    () => {
        "id, slug, label, kind::text, enabled, config, last_test_at, last_test_ok, created_at"
    };
}

async fn load_row(state: &AppState, id: Uuid) -> ApiResult<ProviderRow> {
    sqlx::query_as::<_, ProviderRow>(concat!(
        "SELECT ",
        select_cols!(),
        " FROM auth_providers WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)
}

async fn list(
    State(state): State<AppState>,
    claims: Claims,
) -> ApiResult<Json<Vec<AuthProviderDto>>> {
    require_role(&claims, Role::Admin)?;
    let rows = sqlx::query_as::<_, ProviderRow>(concat!(
        "SELECT ",
        select_cols!(),
        " FROM auth_providers ORDER BY created_at"
    ))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(to_dto).collect()))
}

/// Fusion de la config : on part de l'existante, on superpose les champs
/// fournis ; les champs secrets absents, vides ou masqués ("••••••") gardent
/// leur valeur chiffrée actuelle.
fn merge_config(kind: ProviderKind, old: &Value, new: &Value) -> Value {
    let mut merged = old.clone();
    if let (Some(mo), Some(mn)) = (merged.as_object_mut(), new.as_object()) {
        for (k, v) in mn {
            let keep_old = kind.secret_fields().contains(&k.as_str())
                && (v.is_null()
                    || v.as_str()
                        .map(|s| s.is_empty() || s == auth_providers::MASK)
                        .unwrap_or(false));
            if !keep_old {
                mo.insert(k.clone(), v.clone());
            }
        }
    }
    merged
}

async fn create(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertAuthProviderRequest>,
) -> ApiResult<Json<AuthProviderDto>> {
    require_role(&claims, Role::Admin)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    auth_providers::parse_config(req.kind, &req.config)?;
    let mut config = req.config.clone();
    auth_providers::encrypt_secrets(&state.auth_cipher, req.kind, &mut config)?;
    let row = sqlx::query_as::<_, ProviderRow>(concat!(
        "INSERT INTO auth_providers (slug, label, kind, enabled, config)
         VALUES ($1, $2, $3::auth_provider_kind, $4, $5)
         RETURNING ",
        select_cols!()
    ))
    .bind(&req.slug)
    .bind(&req.label)
    .bind(req.kind.as_str())
    .bind(req.enabled.unwrap_or(true))
    .bind(&config)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("ce slug existe déjà".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "auth_provider.create",
        "auth_provider",
        Some(row.0.to_string()),
    )
    .details(
        json!({"slug": req.slug, "label": req.label, "kind": req.kind.as_str(), "enabled": row.4}),
    )
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(to_dto(row)))
}

async fn update(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertAuthProviderRequest>,
) -> ApiResult<Json<AuthProviderDto>> {
    require_role(&claims, Role::Admin)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let old = load_row(&state, id).await?;
    let old_kind =
        ProviderKind::from_db(&old.3).ok_or(ApiError::Internal("kind inconnu".into()))?;
    if old_kind != req.kind {
        return Err(ApiError::Validation(
            "le type (ldap/oidc) d'un fournisseur ne se change pas".into(),
        ));
    }
    if req.slug != old.1 {
        return Err(ApiError::Validation(
            "le slug ne se change pas (lié aux comptes provisionnés)".into(),
        ));
    }
    let mut config = merge_config(old_kind, &old.5, &req.config);
    auth_providers::parse_config(req.kind, &config)?;
    auth_providers::encrypt_secrets(&state.auth_cipher, req.kind, &mut config)?;
    let row = sqlx::query_as::<_, ProviderRow>(concat!(
        "UPDATE auth_providers SET label = $2, enabled = $3, config = $4 WHERE id = $1
         RETURNING ",
        select_cols!()
    ))
    .bind(id)
    .bind(&req.label)
    .bind(req.enabled.unwrap_or(old.4))
    .bind(&config)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(
        Some(&claims),
        "auth_provider.update",
        "auth_provider",
        Some(id.to_string()),
    )
    .details(
        json!({"slug": req.slug, "label": req.label, "enabled": row.4, "config_changed": true}),
    )
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(to_dto(row)))
}

async fn remove(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let old = load_row(&state, id).await?;
    let result = sqlx::query("DELETE FROM auth_providers WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "auth_provider.delete",
        "auth_provider",
        Some(id.to_string()),
    )
    .details(json!({"slug": old.1, "kind": old.3}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Test de la configuration : bind service (LDAP) ou discovery (OIDC).
async fn test(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ProviderTestResponse>> {
    require_role(&claims, Role::Admin)?;
    let row = load_row(&state, id).await?;
    let kind = ProviderKind::from_db(&row.3).ok_or(ApiError::Internal("kind inconnu".into()))?;
    let config = auth_providers::decrypt_secrets(&state.auth_cipher, kind, &row.5)?;
    let result = match kind {
        ProviderKind::Ldap => {
            let cfg: LdapConfig = serde_json::from_value(config)
                .map_err(|e| ApiError::Validation(format!("config ldap: {e}")))?;
            ldap_auth::test_connection(&cfg).await
        }
        ProviderKind::Oidc => {
            let cfg: OidcConfig = serde_json::from_value(config)
                .map_err(|e| ApiError::Validation(format!("config oidc: {e}")))?;
            oidc_auth::test_discovery(&cfg).await
        }
    };
    let (ok, detail) = match &result {
        Ok(d) => (true, d.clone()),
        Err(e) => (false, e.public_detail()),
    };
    sqlx::query("UPDATE auth_providers SET last_test_at = now(), last_test_ok = $2 WHERE id = $1")
        .bind(id)
        .bind(ok)
        .execute(&state.pool)
        .await?;
    audit::AuditEvent::new(
        Some(&claims),
        "auth_provider.test",
        "auth_provider",
        Some(id.to_string()),
    )
    .details(json!({"slug": row.1, "kind": row.3, "ok": ok}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(ProviderTestResponse { ok, detail }))
}
