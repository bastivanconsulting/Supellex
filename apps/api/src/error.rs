use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Erreur API. Le détail interne est logué côté serveur (tracing) mais jamais
/// renvoyé au client : message générique uniquement (ISO 27001 A.8.32).
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("identifiants invalides")]
    BadCredentials,
    #[error("non authentifié")]
    Unauthorized,
    #[error("droits insuffisants")]
    Forbidden,
    #[error("ressource introuvable")]
    NotFound,
    #[error("conflit : {0}")]
    Conflict(String),
    #[error("donnée invalide : {0}")]
    Validation(String),
    #[error("trop de tentatives, réessayez plus tard")]
    TooManyRequests,
    /// Erreur de pilotage d'un appareil réseau (message contrôlé, sans secret).
    #[error("appareil injoignable ou commande refusée : {0}")]
    Device(String),
    /// Service externe (annuaire LDAP, fournisseur SSO) injoignable ou en erreur.
    #[error("service externe indisponible : {0}")]
    BadGateway(String),
    #[error("erreur interne")]
    Internal(String),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::BadCredentials | Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::Device(_) | Self::BadGateway(_) => StatusCode::BAD_GATEWAY,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Corps exposé au client : jamais de détail interne.
    fn public_message(&self) -> String {
        match self {
            Self::Internal(_) => "erreur interne".to_string(),
            other => other.to_string(),
        }
    }

    /// Détail exposé pour les diagnostics admin (bouton « Tester » des
    /// fournisseurs d'auth) : les variantes contrôlées (Device, Validation,
    /// Conflict) livrent leur message ; Internal reste générique.
    pub fn public_detail(&self) -> String {
        match self {
            Self::Internal(_) => "erreur interne".to_string(),
            other => other.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if matches!(self, Self::Internal(_)) {
            tracing::error!(error = %self, "erreur interne");
        }
        let status = self.status();
        (status, Json(json!({ "error": self.public_message() }))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => Self::NotFound,
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                Self::Conflict("la ressource existe déjà".into())
            }
            other => Self::Internal(other.to_string()),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
