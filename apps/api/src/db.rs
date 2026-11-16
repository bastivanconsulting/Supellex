use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::auth::{Role, hash_password};
use crate::config::Config;

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(database_url)
        .await
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

/// Crée le compte admin initial si aucun utilisateur n'existe (démarrage à froid).
/// Le mot de passe est hashé en argon2id, jamais stocké en clair.
pub async fn seed_admin(pool: &PgPool, cfg: &Config) -> Result<(), sqlx::Error> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if count.0 > 0 {
        return Ok(());
    }
    let hash =
        hash_password(&cfg.admin_password).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    sqlx::query(
        "INSERT INTO users (email, display_name, password_hash, role, must_change_password)
         VALUES ($1, 'Administrateur', $2, 'admin', TRUE)",
    )
    .bind(&cfg.admin_email)
    .bind(hash)
    .execute(pool)
    .await?;
    tracing::warn!(
        email = %cfg.admin_email,
        "compte admin initial créé avec mot de passe à changer obligatoirement"
    );
    Ok(())
}

/// Rôle lu en base pour un utilisateur (source de vérité, pas le JWT).
pub async fn role_of(pool: &PgPool, user_id: uuid::Uuid) -> Result<Option<Role>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT role::text FROM users WHERE id = $1 AND NOT disabled")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(r,)| Role::from_db(&r)))
}

/// Paramètres d'une connexion externe (LDAP/OIDC).
pub struct ExternalLogin {
    pub email: String,
    pub display_name: String,
    /// Rôle déduit du mapping (None si aucun groupe/claim ne matche).
    pub desired_role: Option<Role>,
    pub default_role: Role,
    /// slug du fournisseur (colonne users.auth_source).
    pub auth_source: String,
    pub sync_roles: bool,
    pub jit_provision: bool,
}

/// Compte pour une connexion externe (LDAP/OIDC) : recherche par e-mail,
/// création JIT si inconnue (hash de mot de passe aléatoire inutilisable).
/// `desired_role` est appliqué aux comptes existants si `sync_roles`, sinon
/// `default_role` à la création. Retourne None si le compte est désactivé ou
/// inconnu sans JIT. Retourne (id, rôle effectif, must_change_password).
pub async fn resolve_external_user(
    pool: &PgPool,
    login: &ExternalLogin,
) -> Result<Option<(uuid::Uuid, Role, bool)>, sqlx::Error> {
    let email = &login.email;
    let display_name = &login.display_name;
    let desired_role = login.desired_role;
    let default_role = login.default_role;
    let auth_source = &login.auth_source;
    let sync_roles = login.sync_roles;
    let jit_provision = login.jit_provision;
    let existing: Option<(uuid::Uuid, String, bool, bool)> = sqlx::query_as(
        "SELECT id, role::text, must_change_password, disabled FROM users WHERE email = $1",
    )
    .bind(email.to_lowercase())
    .fetch_optional(pool)
    .await?;
    match existing {
        Some((id, db_role, must_change, disabled)) => {
            if disabled {
                return Ok(None);
            }
            let current = Role::from_db(&db_role).unwrap_or(Role::Viewer);
            let role = if sync_roles {
                desired_role.unwrap_or(current)
            } else {
                current
            };
            // Protection : ne jamais rétrograder le dernier admin actif.
            if current == Role::Admin && role != Role::Admin {
                let (count,): (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM users WHERE role = 'admin' AND NOT disabled",
                )
                .fetch_one(pool)
                .await?;
                if count <= 1 {
                    return Ok(Some((id, current, must_change)));
                }
            }
            if role != current {
                sqlx::query("UPDATE users SET role = $2::user_role WHERE id = $1")
                    .bind(id)
                    .bind(role.as_str())
                    .execute(pool)
                    .await?;
            }
            Ok(Some((id, role, must_change)))
        }
        None => {
            if !jit_provision {
                return Ok(None);
            }
            let role = desired_role.unwrap_or(default_role);
            // Mot de passe local inutilisable : hash d'une valeur aléatoire.
            let random_pw = {
                use rand_core::{OsRng, RngCore};
                let mut b = [0u8; 32];
                OsRng.fill_bytes(&mut b);
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b)
            };
            let hash =
                hash_password(&random_pw).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
            let row: Option<(uuid::Uuid,)> = sqlx::query_as(
                "INSERT INTO users (email, display_name, password_hash, role, must_change_password, auth_source)
                 VALUES ($1, $2, $3, $4::user_role, FALSE, $5)
                 ON CONFLICT (email) DO NOTHING
                 RETURNING id",
            )
            .bind(email.to_lowercase())
            .bind(display_name)
            .bind(hash)
            .bind(role.as_str())
            .bind(auth_source)
            .fetch_optional(pool)
            .await?;
            match row {
                Some((id,)) => Ok(Some((id, role, false))),
                None => {
                    // créé entre-temps : on relit
                    let (id, r, m): (uuid::Uuid, String, bool) = sqlx::query_as(
                        "SELECT id, role::text, must_change_password FROM users WHERE email = $1",
                    )
                    .bind(email.to_lowercase())
                    .fetch_one(pool)
                    .await?;
                    Ok(Some((id, Role::from_db(&r).unwrap_or(Role::Viewer), m)))
                }
            }
        }
    }
}
