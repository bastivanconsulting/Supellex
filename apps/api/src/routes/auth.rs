//! Authentification : login local (argon2id) ou via une source externe
//! configurée (LDAP via bind, SSO/OIDC via authorization code + PKCE),
//! liste publique des sources (page de login), gestion des comptes.

use axum::{
    Json, Router,
    extract::{ConnectInfo, FromRequestParts, Path, Query, State},
    http::request::Parts,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use crate::{
    audit,
    auth::{self, Claims, Role, require_role},
    auth_providers::{self, LdapConfig, OidcConfig, ProviderKind},
    db,
    error::{ApiError, ApiResult},
    ldap_auth,
    models::*,
    oidc_auth,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/providers", get(public_providers))
        .route("/oidc/{slug}/start", get(oidc_start))
        .route("/oidc/callback", get(oidc_callback))
}

pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route("/me", get(me))
        .route("/change-password", post(change_password))
}

pub fn users_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users).post(create_user))
        .route(
            "/{id}",
            axum::routing::patch(update_user).delete(delete_user),
        )
}

/// Charge un fournisseur actif par slug avec sa config déchiffrée.
async fn load_enabled_provider(
    state: &AppState,
    slug: &str,
) -> ApiResult<(Uuid, ProviderKind, String, Value)> {
    let row: Option<(Uuid, String, String, Value)> = sqlx::query_as(
        "SELECT id, kind::text, label, config FROM auth_providers WHERE slug = $1 AND enabled",
    )
    .bind(slug)
    .fetch_optional(&state.pool)
    .await?;
    let (id, kind, label, config) = row.ok_or(ApiError::NotFound)?;
    let kind = ProviderKind::from_db(&kind).ok_or(ApiError::Internal("kind inconnu".into()))?;
    let config = auth_providers::decrypt_secrets(&state.auth_cipher, kind, &config)?;
    Ok((id, kind, label, config))
}

/// Émet la session (JWT + audit + last_login) pour un compte authentifié,
/// quelle que soit la source.
async fn issue_session(
    state: &AppState,
    user_id: Uuid,
    email: &str,
    role: Role,
    must_change: bool,
    client_ip: Option<std::net::IpAddr>,
    source: &str,
) -> ApiResult<Json<LoginResponse>> {
    let token = state
        .keys
        .issue(user_id, email, role, state.jwt_ttl_seconds)?;
    sqlx::query("UPDATE users SET last_login_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    audit::AuditEvent::new(
        Some(&Claims {
            sub: user_id,
            email: email.to_string(),
            role,
            exp: 0,
            iat: 0,
        }),
        "auth.login",
        "user",
        Some(user_id.to_string()),
    )
    .details(json!({"source": source}))
    .ip(client_ip)
    .write(&state.pool)
    .await;
    Ok(Json(LoginResponse {
        token,
        token_type: "Bearer",
        expires_in: state.jwt_ttl_seconds,
        must_change_password: must_change,
    }))
}

async fn login(
    State(state): State<AppState>,
    ConnectInfoExtract(client_ip): ConnectInfoExtract,
    Json(req): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    // Rate limiting anti force brute par IP client (fenêtre glissante),
    // quelle que soit la source d'auth.
    if state.login_limiter.hit(client_ip).await.is_err() {
        audit::AuditEvent::new(None, "auth.login_rate_limited", "user", None)
            .details(json!({"email": req.email, "source": req.source}))
            .ip(Some(client_ip))
            .write(&state.pool)
            .await;
        return Err(ApiError::TooManyRequests);
    }
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let source = req.source.clone().unwrap_or_else(|| "local".to_string());
    if source == "local" {
        if !req.email.contains('@') {
            return Err(ApiError::Validation("e-mail invalide".into()));
        }
        return login_local(&state, &req, client_ip).await;
    }

    // Source externe : LDAP uniquement ici (OIDC passe par /auth/oidc/*).
    let (_id, kind, _label, config) = load_enabled_provider(&state, &source).await?;
    if kind != ProviderKind::Ldap {
        return Err(ApiError::Validation(
            "cette source se connecte via son bouton SSO, pas par formulaire".into(),
        ));
    }
    let cfg: LdapConfig = serde_json::from_value(config)
        .map_err(|e| ApiError::Internal(format!("config ldap: {e}")))?;
    let info = match ldap_auth::authenticate(&cfg, &req.email, &req.password).await {
        Ok(i) => i,
        Err(e) => {
            audit::AuditEvent::new(None, "auth.login_failed", "user", None)
                .details(json!({"email": req.email, "source": source}))
                .ip(Some(client_ip))
                .write(&state.pool)
                .await;
            return Err(e);
        }
    };
    let desired = auth_providers::mapped_role(&cfg.role_map, &info.groups);
    let resolved = db::resolve_external_user(
        &state.pool,
        &db::ExternalLogin {
            email: info.email.clone(),
            display_name: info.display_name.clone(),
            desired_role: desired,
            default_role: cfg.default_role,
            auth_source: source.clone(),
            sync_roles: cfg.sync_roles,
            jit_provision: cfg.jit_provision,
        },
    )
    .await?;
    match resolved {
        Some((id, role, must_change)) => {
            issue_session(
                &state,
                id,
                &info.email,
                role,
                must_change,
                Some(client_ip),
                &source,
            )
            .await
        }
        None => {
            audit::AuditEvent::new(None, "auth.login_failed", "user", None)
                .details(json!({"email": info.email, "source": source, "reason": "compte désactivé ou JIT désactivé"}))
                .ip(Some(client_ip))
                .write(&state.pool)
                .await;
            Err(ApiError::BadCredentials)
        }
    }
}

async fn login_local(
    state: &AppState,
    req: &LoginRequest,
    client_ip: std::net::IpAddr,
) -> ApiResult<Json<LoginResponse>> {
    let row = sqlx::query_as::<_, (Uuid, String, String, String, bool, bool)>(
        "SELECT id, email::text, password_hash, role::text, disabled, must_change_password
         FROM users WHERE email = $1",
    )
    .bind(req.email.to_lowercase())
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::BadCredentials)?;

    let (id, email, hash, role, disabled, must_change) = row;
    if disabled || !auth::verify_password(&req.password, &hash) {
        audit::AuditEvent::new(None, "auth.login_failed", "user", Some(id.to_string()))
            .details(json!({"email": req.email, "source": "local"}))
            .ip(Some(client_ip))
            .write(&state.pool)
            .await;
        return Err(ApiError::BadCredentials);
    }
    let role = Role::from_db(&role).ok_or(ApiError::Internal("rôle inconnu".into()))?;
    issue_session(
        state,
        id,
        &email,
        role,
        must_change,
        Some(client_ip),
        "local",
    )
    .await
}

/// Liste publique des sources d'auth actives (page de login).
async fn public_providers(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<PublicAuthProvider>>> {
    let mut out = vec![PublicAuthProvider {
        slug: "local".into(),
        label: "Compte local".into(),
        kind: "local".into(),
        login_label: None,
    }];
    let rows: Vec<(String, String, String, Value)> = sqlx::query_as(
        "SELECT slug, label, kind::text, config FROM auth_providers WHERE enabled ORDER BY label",
    )
    .fetch_all(&state.pool)
    .await?;
    for (slug, label, kind, config) in rows {
        let login_label = if kind == "ldap" {
            serde_json::from_value::<LdapConfig>(config.clone())
                .ok()
                .map(|c| c.login_attribute)
        } else {
            None
        };
        out.push(PublicAuthProvider {
            slug,
            label,
            kind,
            login_label,
        });
    }
    Ok(Json(out))
}

/// Démarre un flux OIDC : crée la session (state/PKCE/nonce, 10 min) et
/// renvoie l'URL d'autorisation du fournisseur.
async fn oidc_start(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<OidcStartResponse>> {
    // Ménage des sessions expirées (opportuniste).
    let _ = sqlx::query("DELETE FROM oidc_login_sessions WHERE expires_at < now()")
        .execute(&state.pool)
        .await;
    let (provider_id, kind, _label, config) = load_enabled_provider(&state, &slug).await?;
    if kind != ProviderKind::Oidc {
        return Err(ApiError::NotFound);
    }
    let cfg: OidcConfig = serde_json::from_value(config)
        .map_err(|e| ApiError::Internal(format!("config oidc: {e}")))?;
    let redirect_uri = cfg
        .redirect_uri
        .clone()
        .unwrap_or_else(|| format!("{}/api/auth/oidc/callback", state.public_base_url));
    let started = oidc_auth::start(&cfg, &redirect_uri).await?;
    sqlx::query(
        "INSERT INTO oidc_login_sessions (state, provider_id, nonce, pkce_verifier, expires_at)
         VALUES ($1, $2, $3, $4, now() + interval '10 minutes')",
    )
    .bind(&started.state)
    .bind(provider_id)
    .bind(&started.nonce)
    .bind(&started.pkce_verifier)
    .execute(&state.pool)
    .await?;
    Ok(Json(OidcStartResponse {
        authorization_url: started.authorization_url,
    }))
}

#[derive(Debug, Deserialize)]
struct OidcCallbackQuery {
    code: String,
    state: String,
}

/// Callback OIDC (appelé par le navigateur, redirect_uri du fournisseur) :
/// échange le code, vérifie l'id_token, provisionne le compte, émet le JWT.
async fn oidc_callback(
    State(state): State<AppState>,
    ConnectInfoExtract(client_ip): ConnectInfoExtract,
    Query(q): Query<OidcCallbackQuery>,
) -> ApiResult<Json<LoginResponse>> {
    // Session one-shot : consommée immédiatement.
    let session: Option<(Uuid, String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "DELETE FROM oidc_login_sessions WHERE state = $1
         RETURNING provider_id, nonce, pkce_verifier, expires_at",
    )
    .bind(&q.state)
    .fetch_optional(&state.pool)
    .await?;
    let (provider_id, nonce, pkce_verifier, expires_at) = session.ok_or(ApiError::Unauthorized)?;
    if expires_at < chrono::Utc::now() {
        return Err(ApiError::Unauthorized);
    }
    let row: Option<(String, String, String, Value)> = sqlx::query_as(
        "SELECT slug, label, kind::text, config FROM auth_providers WHERE id = $1 AND enabled",
    )
    .bind(provider_id)
    .fetch_optional(&state.pool)
    .await?;
    let (slug, _label, kind, config) = row.ok_or(ApiError::Unauthorized)?;
    if kind != "oidc" {
        return Err(ApiError::Unauthorized);
    }
    let config = auth_providers::decrypt_secrets(&state.auth_cipher, ProviderKind::Oidc, &config)?;
    let cfg: OidcConfig = serde_json::from_value(config)
        .map_err(|e| ApiError::Internal(format!("config oidc: {e}")))?;
    let redirect_uri = cfg
        .redirect_uri
        .clone()
        .unwrap_or_else(|| format!("{}/api/auth/oidc/callback", state.public_base_url));
    let claims =
        match oidc_auth::exchange(&cfg, &redirect_uri, &q.code, &pkce_verifier, &nonce).await {
            Ok(c) => c,
            Err(e) => {
                audit::AuditEvent::new(
                    None,
                    "auth.login_failed",
                    "auth_provider",
                    Some(slug.clone()),
                )
                .details(json!({"source": slug, "stage": "oidc_exchange"}))
                .ip(Some(client_ip))
                .write(&state.pool)
                .await;
                return Err(e);
            }
        };
    let desired = auth_providers::mapped_role(&cfg.role_map, &claims.roles);
    let resolved = db::resolve_external_user(
        &state.pool,
        &db::ExternalLogin {
            email: claims.email.clone(),
            display_name: claims.display_name.clone(),
            desired_role: desired,
            default_role: cfg.default_role,
            auth_source: slug.clone(),
            sync_roles: cfg.sync_roles,
            jit_provision: cfg.jit_provision,
        },
    )
    .await?;
    match resolved {
        Some((id, role, must_change)) => {
            issue_session(
                &state,
                id,
                &claims.email,
                role,
                must_change,
                Some(client_ip),
                &slug,
            )
            .await
        }
        None => {
            audit::AuditEvent::new(None, "auth.login_failed", "user", None)
                .details(json!({"email": claims.email, "source": slug, "reason": "compte désactivé ou JIT désactivé"}))
                .ip(Some(client_ip))
                .write(&state.pool)
                .await;
            Err(ApiError::BadCredentials)
        }
    }
}

async fn me(State(_state): State<AppState>, claims: Claims) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({
        "id": claims.sub,
        "email": claims.email,
        "role": claims.role,
    })))
}

async fn change_password(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<ChangePasswordRequest>,
) -> ApiResult<axum::http::StatusCode> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let (hash,): (String,) = sqlx::query_as("SELECT password_hash FROM users WHERE id = $1")
        .bind(claims.sub)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    if !auth::verify_password(&req.old_password, &hash) {
        return Err(ApiError::BadCredentials);
    }
    let new_hash = auth::hash_password(&req.new_password)?;
    sqlx::query("UPDATE users SET password_hash = $1, must_change_password = FALSE WHERE id = $2")
        .bind(new_hash)
        .bind(claims.sub)
        .execute(&state.pool)
        .await?;
    audit::AuditEvent::new(
        Some(&claims),
        "auth.password_changed",
        "user",
        Some(claims.sub.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn list_users(
    State(state): State<AppState>,
    claims: Claims,
) -> ApiResult<Json<Vec<UserDto>>> {
    require_role(&claims, Role::Admin)?;
    let rows = sqlx::query_as::<_, (Uuid, String, String, String, bool, bool, String, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>)>(
        "SELECT id, email::text, display_name, role::text, disabled, must_change_password, auth_source, created_at, last_login_at
         FROM users ORDER BY email",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    email,
                    display_name,
                    role,
                    disabled,
                    must_change,
                    auth_source,
                    created_at,
                    last_login,
                )| {
                    UserDto {
                        id,
                        email,
                        display_name,
                        role: Role::from_db(&role).unwrap_or(Role::Viewer),
                        disabled,
                        must_change_password: must_change,
                        auth_source,
                        created_at,
                        last_login_at: last_login,
                    }
                },
            )
            .collect(),
    ))
}

async fn create_user(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<CreateUserRequest>,
) -> ApiResult<Json<UserDto>> {
    require_role(&claims, Role::Admin)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let hash = auth::hash_password(&req.password)?;
    let row = sqlx::query_as::<_, (Uuid, String, String, String, bool, bool, String, chrono::DateTime<chrono::Utc>)>(
        "INSERT INTO users (email, display_name, password_hash, role, must_change_password)
         VALUES ($1, $2, $3, $4::user_role, TRUE)
         RETURNING id, email::text, display_name, role::text, disabled, must_change_password, auth_source, created_at",
    )
    .bind(req.email.to_lowercase())
    .bind(&req.display_name)
    .bind(hash)
    .bind(req.role.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("existe déjà".into()))?;
    audit::AuditEvent::new(
        Some(&claims),
        "user.create",
        "user",
        Some(row.0.to_string()),
    )
    .details(json!({"email": req.email, "role": req.role}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(UserDto {
        id: row.0,
        email: row.1,
        display_name: row.2,
        role: Role::from_db(&row.3).unwrap_or(Role::Viewer),
        disabled: row.4,
        must_change_password: row.5,
        auth_source: row.6,
        created_at: row.7,
        last_login_at: None,
    }))
}

async fn update_user(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateUserRequest>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if id == claims.sub && req.role.is_some() {
        return Err(ApiError::Validation(
            "impossible de changer son propre rôle".into(),
        ));
    }
    let result = sqlx::query(
        "UPDATE users SET
           display_name = COALESCE($2, display_name),
           role = COALESCE($3::user_role, role),
           disabled = COALESCE($4, disabled)
         WHERE id = $1",
    )
    .bind(id)
    .bind(req.display_name)
    .bind(req.role.map(|r| r.as_str().to_string()))
    .bind(req.disabled)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(Some(&claims), "user.update", "user", Some(id.to_string()))
        .details(json!({"role": req.role, "disabled": req.disabled}))
        .ip(None)
        .write(&state.pool)
        .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn delete_user(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    if id == claims.sub {
        return Err(ApiError::Validation(
            "impossible de supprimer son propre compte".into(),
        ));
    }
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(Some(&claims), "user.delete", "user", Some(id.to_string()))
        .details(json!({}))
        .ip(None)
        .write(&state.pool)
        .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// Extracteur d'IP client léger (évite de traîner ConnectInfo partout).
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
pub struct ConnectInfoExtract(pub IpAddr);

impl<S> FromRequestParts<S> for ConnectInfoExtract
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // En production, l'API n'est joignable que par le BFF Next.js, qui
        // forward l'IP réelle du navigateur via X-Forwarded-For (première
        // valeur). Repli sur l'IP de la connexion TCP directe sinon.
        let ip = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next()?.trim().parse::<IpAddr>().ok())
            .or_else(|| {
                parts
                    .extensions
                    .get::<ConnectInfo<SocketAddr>>()
                    .map(|c| c.0.ip())
            })
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        Ok(ConnectInfoExtract(ip))
    }
}
