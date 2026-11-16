use axum::{Extension, RequestPartsExt, extract::FromRequestParts, http::request::Parts};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer = 1,
    Operator = 2,
    Admin = 3,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Operator => "operator",
            Role::Admin => "admin",
        }
    }
    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "viewer" => Some(Role::Viewer),
            "operator" => Some(Role::Operator),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid, // user id
    pub email: String,
    pub role: Role,
    pub exp: i64,
    pub iat: i64,
}

#[derive(Clone)]
pub struct JwtKeys {
    enc: EncodingKey,
    dec: DecodingKey,
}

impl JwtKeys {
    pub fn new(secret: &[u8]) -> Result<Self, ApiError> {
        Ok(Self {
            enc: EncodingKey::from_secret(secret)
                .map_err(|e| ApiError::Internal(format!("clé JWT: {e}")))?,
            dec: DecodingKey::from_secret(secret)
                .map_err(|e| ApiError::Internal(format!("clé JWT: {e}")))?,
        })
    }

    pub fn issue(
        &self,
        user_id: Uuid,
        email: &str,
        role: Role,
        ttl_seconds: u64,
    ) -> Result<String, ApiError> {
        let now = Utc::now();
        let claims = Claims {
            sub: user_id,
            email: email.to_string(),
            role,
            iat: now.timestamp(),
            exp: (now + Duration::seconds(ttl_seconds as i64)).timestamp(),
        };
        encode(&Header::default(), &claims, &self.enc)
            .map_err(|e| ApiError::Internal(format!("signature JWT: {e}")))
    }

    pub fn verify(&self, token: &str) -> Result<Claims, ApiError> {
        let data = decode::<Claims>(token, &self.dec, &Validation::default())
            .map_err(|_| ApiError::Unauthorized)?;
        Ok(data.claims)
    }
}

/// Hash / vérification de mot de passe : argon2id (standard, jamais de crypto maison).
/// Le sel aléatoire est généré en interne par password-hash (getrandom).
pub fn hash_password(password: &str) -> Result<String, ApiError> {
    use password_hash::PasswordHasher;
    argon2::Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| ApiError::Internal(format!("hash mot de passe: {e}")))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    use password_hash::{PasswordVerifier, phc::PasswordHash};
    match hash.parse::<PasswordHash>() {
        Ok(parsed) => argon2::Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Extracteur axum : exige un JWT Bearer valide. Refus par défaut.
impl<S> FromRequestParts<S> for Claims
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Extension(state) = parts
            .extract::<Extension<AppState>>()
            .await
            .map_err(|_| ApiError::Internal("state absent".into()))?;
        let TypedHeader(authorization) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| ApiError::Unauthorized)?;
        let claims = state.keys.verify(authorization.token())?;
        // La signature ne suffit pas : le compte doit toujours exister et être
        // actif, et son rôle en base doit correspondre au rôle du token.
        // Ainsi, désactiver un compte ou changer son rôle révoque immédiatement
        // tous ses tokens encore valides (la base est la source de vérité).
        let db_role = crate::db::role_of(&state.pool, claims.sub)
            .await
            .map_err(|_| ApiError::Internal("vérification du compte".into()))?
            .ok_or(ApiError::Unauthorized)?;
        if db_role != claims.role {
            return Err(ApiError::Unauthorized);
        }
        Ok(Claims {
            role: db_role,
            ..claims
        })
    }
}

/// Vérifie un rôle minimal. Ordre : viewer < operator < admin.
pub fn require_role(claims: &Claims, min: Role) -> Result<(), ApiError> {
    if claims.role >= min {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}
