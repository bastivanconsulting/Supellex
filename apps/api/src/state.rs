use sqlx::PgPool;

use crate::auth::JwtKeys;
use crate::crypto::SecretCipher;
use crate::ratelimit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub keys: JwtKeys,
    pub jwt_ttl_seconds: u64,
    /// Timeout d'un ping en millisecondes.
    pub ping_timeout_ms: u64,
    /// Nombre de pings simultanés (scan / balayage).
    pub ping_concurrency: usize,
    /// Nombre max d'hôtes balayés sur un scan à la demande.
    pub scan_max_hosts: u128,
    /// Rate limiting des tentatives de login (par IP client).
    pub login_limiter: RateLimiter,
    /// Chiffrement au repos des secrets des appareils réseau.
    pub device_cipher: SecretCipher,
    /// Chiffrement au repos des secrets des sources d'auth (AAD distinct).
    pub auth_cipher: SecretCipher,
    /// URL publique du front (redirect_uri OIDC par défaut).
    pub public_base_url: String,
}
