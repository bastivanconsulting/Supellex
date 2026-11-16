pub mod assets;
pub mod audit;
pub mod auth;
pub mod auth_providers;
pub mod catalog;
pub mod devices;
pub mod discovery;
pub mod ipam;
pub mod locations;
pub mod people;

use axum::Router;

use crate::state::AppState;

/// Routes publiques (pas de JWT requis).
pub fn public_routes() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::router())
        .route("/health", axum::routing::get(|| async { "ok" }))
}

/// Routes protégées (JWT requis, vérifié par l'extracteur Claims).
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::protected_router())
        .nest("/users", auth::users_router())
        .nest("/people", people::router())
        .nest("/locations", locations::router())
        .nest("/assets", assets::router())
        .nest("/ipam", ipam::router())
        .nest("/ipam/vlan-domains", catalog::domains_router())
        .nest("/ipam/tags", catalog::tags_router())
        .nest("/ipam/dhcp", catalog::dhcp_router())
        .nest("/ipam/nat", catalog::nat_router())
        .nest("/ipam/fields", catalog::fields_router())
        .nest("/search", catalog::search_router())
        .nest("/audit", audit::router())
        .nest("/devices", devices::router())
        .nest("/auth-providers", auth_providers::router())
}
