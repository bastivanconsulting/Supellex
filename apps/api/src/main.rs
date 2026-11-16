#![forbid(unsafe_code)]

mod audit;
mod auth;
mod auth_providers;
mod config;
mod crypto;
mod db;
mod device_net;
mod error;
mod ipam_logic;
mod ldap_auth;
mod models;
mod oidc_auth;
mod ping;
mod provision;
mod ratelimit;
mod routes;
mod state;

use std::net::SocketAddr;
use std::time::Duration;

use axum::{Extension, Router, middleware};
use tower_http::{
    cors::CorsLayer, limit::RequestBodyLimitLayer, set_header::SetResponseHeaderLayer,
    timeout::TimeoutLayer, trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::auth::JwtKeys;
use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,supellex_api=debug,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    dotenvy::dotenv().ok();
    let cfg = Config::from_env()?;

    let pool = db::connect(&cfg.database_url).await?;
    db::run_migrations(&pool).await?;
    db::seed_admin(&pool, &cfg).await?;

    let state = AppState {
        pool,
        keys: JwtKeys::new(&cfg.jwt_secret)?,
        jwt_ttl_seconds: cfg.jwt_ttl_seconds,
        ping_timeout_ms: cfg.ping_timeout_ms,
        ping_concurrency: cfg.ping_concurrency,
        scan_max_hosts: cfg.scan_max_hosts,
        login_limiter: ratelimit::RateLimiter::new(
            cfg.login_max_attempts,
            Duration::from_secs(cfg.login_window_seconds),
        ),
        device_cipher: crypto::SecretCipher::new(&cfg.devices_encryption_key),
        auth_cipher: crypto::SecretCipher::with_aad(&cfg.devices_encryption_key, crypto::AUTH_AAD),
        public_base_url: cfg.public_base_url.clone(),
    };

    // Balayage ping automatique des sous-réseaux (ping_scan) en arrière-plan.
    tokio::spawn(ping::run_background_loop(
        state.pool.clone(),
        Duration::from_secs(cfg.ping_interval_seconds),
        cfg.ping_timeout_ms,
        cfg.ping_concurrency,
    ));

    // En-têtes de sécurité (ISO 27001 A.8.29 / bonnes pratiques web)
    let security_headers = tower::ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::CONTENT_SECURITY_POLICY,
            axum::http::HeaderValue::from_static(
                "default-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'",
            ),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            axum::http::HeaderValue::from_static("max-age=63072000; includeSubDomains"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::REFERRER_POLICY,
            axum::http::HeaderValue::from_static("no-referrer"),
        ));

    let allowed: Vec<axum::http::HeaderValue> = cfg
        .cors_allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();
    let fallback = axum::http::HeaderValue::from_static("http://localhost:3000");
    let allow_origin = if allowed.is_empty() {
        tower_http::cors::AllowOrigin::exact(fallback)
    } else {
        tower_http::cors::AllowOrigin::from(allowed)
    };
    let cors = CorsLayer::new()
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
        ])
        .allow_origin(allow_origin);

    let app = Router::new()
        .merge(routes::public_routes())
        .merge(routes::protected_routes())
        .route_layer(middleware::from_fn(correlation_id_middleware))
        .layer(security_headers)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        // Corps de requête borné à 1 Mo (anti-abus)
        .layer(RequestBodyLimitLayer::new(1_048_576))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .with_state(state.clone())
        .layer(Extension(state));

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "supellex-api démarré");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Ajoute un identifiant de corrélation à chaque requête (traçabilité ISO 27001 A.8.15).
async fn correlation_id_middleware(
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let id = uuid::Uuid::new_v4();
    req.extensions_mut().insert(id);
    let mut response = next.run(req).await;
    if let Ok(v) = axum::http::HeaderValue::from_str(&id.to_string()) {
        response.headers_mut().insert("x-correlation-id", v);
    }
    response
}
