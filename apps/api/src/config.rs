use std::env;

/// Configuration chargée depuis l'environnement. Aucun secret en dur :
/// chaque valeur provient d'une variable d'environnement (voir .env.example,
/// à générer aléatoirement, ex. openssl rand -base64 32).
#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: Vec<u8>,
    pub jwt_ttl_seconds: u64,
    pub admin_email: String,
    pub admin_password: String,
    pub port: u16,
    pub cors_allowed_origins: Vec<String>,
    /// Intervalle du balayage ping automatique (0 = désactivé).
    pub ping_interval_seconds: u64,
    /// Timeout d'un ping en millisecondes.
    pub ping_timeout_ms: u64,
    /// Nombre de pings simultanés.
    pub ping_concurrency: usize,
    /// Nombre max d'hôtes scannés sur un scan à la demande.
    pub scan_max_hosts: u128,
    /// Nombre max de tentatives de login par IP sur la fenêtre.
    pub login_max_attempts: usize,
    /// Fenêtre glissante du rate limiting de login, en secondes.
    pub login_window_seconds: u64,
    /// Clé AES-256 (32 octets) de chiffrement au repos des secrets appareils.
    pub devices_encryption_key: [u8; 32],
    /// URL publique du front (BFF), ex. https://supellex.example.com. Sert à
    /// construire le redirect_uri OIDC par défaut : <PUBLIC_BASE_URL>/api/auth/oidc/callback.
    pub public_base_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("variable d'environnement manquante : {0}")]
    Missing(String),
    #[error("variable d'environnement invalide : {0}")]
    Invalid(String),
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        fn req(name: &str) -> Result<String, ConfigError> {
            env::var(name).map_err(|_| ConfigError::Missing(name.to_string()))
        }
        fn placeholder(name: &str, value: &str) -> Result<(), ConfigError> {
            if value.starts_with("CHANGER_MOI") {
                return Err(ConfigError::Invalid(format!(
                    "{name} contient encore la valeur d'exemple - générer une valeur aléatoire réelle"
                )));
            }
            Ok(())
        }

        let database_url = req("DATABASE_URL")?;
        let jwt_secret = req("JWT_SECRET")?;
        placeholder("JWT_SECRET", &jwt_secret)?;
        if jwt_secret.len() < 32 {
            return Err(ConfigError::Invalid(
                "JWT_SECRET doit faire au moins 32 caractères".into(),
            ));
        }
        let admin_password = req("ADMIN_INITIAL_PASSWORD")?;
        placeholder("ADMIN_INITIAL_PASSWORD", &admin_password)?;

        let jwt_ttl_seconds = env::var("JWT_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);

        let port = env::var("API_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);

        let cors_allowed_origins = env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:3000".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let ping_interval_seconds = env::var("PING_INTERVAL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        let ping_timeout_ms = env::var("PING_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000);
        let ping_concurrency = env::var("PING_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(32);
        let scan_max_hosts = env::var("SCAN_MAX_HOSTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4096u128);

        let login_max_attempts = env::var("LOGIN_MAX_ATTEMPTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        let login_window_seconds = env::var("LOGIN_WINDOW_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);

        // Clé de chiffrement des secrets appareils et fournisseurs d'auth
        // (base64 de 32 octets, ex. openssl rand -base64 32). Absente en dev :
        // clé éphémère + warning,
        // les secrets ne survivent alors pas au redémarrage.
        let devices_encryption_key: [u8; 32] = match env::var("DEVICES_ENCRYPTION_KEY") {
            Ok(v) if !v.starts_with("CHANGER_MOI") => {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                let bytes = STANDARD.decode(v.trim()).map_err(|_| {
                    ConfigError::Invalid(
                        "DEVICES_ENCRYPTION_KEY doit être du base64 de 32 octets".into(),
                    )
                })?;
                <[u8; 32]>::try_from(bytes).map_err(|_| {
                    ConfigError::Invalid("DEVICES_ENCRYPTION_KEY doit faire 32 octets".into())
                })?
            }
            _ => {
                use rand_core::{OsRng, RngCore};
                let mut k = [0u8; 32];
                OsRng.fill_bytes(&mut k);
                eprintln!(
                    "WARN DEVICES_ENCRYPTION_KEY absent : clé éphémère, secrets appareils perdus au redémarrage"
                );
                k
            }
        };

        Ok(Self {
            database_url,
            jwt_secret: jwt_secret.into_bytes(),
            jwt_ttl_seconds,
            admin_email: env::var("ADMIN_EMAIL").unwrap_or_else(|_| "admin@supellex.local".into()),
            admin_password,
            port,
            cors_allowed_origins,
            ping_interval_seconds,
            ping_timeout_ms,
            ping_concurrency,
            scan_max_hosts,
            login_max_attempts,
            login_window_seconds,
            devices_encryption_key,
            public_base_url: env::var("PUBLIC_BASE_URL")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "http://localhost:3000".into())
                .trim_end_matches('/')
                .to_string(),
        })
    }
}
