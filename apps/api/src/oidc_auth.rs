//! SSO / OpenID Connect : flux authorization code + PKCE, vérification de
//! l'id_token (signature via JWKS, iss, aud, exp, nonce) via la crate
//! openidconnect. Les claims métier (e-mail, rôles) sont lus dans le payload
//! de l'id_token après vérification. Jamais de secret logué.

use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse as _,
    reqwest::{self, redirect::Policy},
};

/// Client après discovery : endpoint auth renseigné, token/userinfo optionnels
/// (marqueurs oauth2 v5, conforme au type construit par from_provider_metadata).
type DiscoveredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;
use serde_json::Value;

use crate::auth_providers::{OidcConfig, dotted_get, roles_from_claim};
use crate::error::ApiError;

/// Résultat du démarrage d'un flux OIDC (à persister côté appelant).
pub struct OidcStart {
    pub authorization_url: String,
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
}

/// Claims extraits d'un id_token vérifié.
pub struct OidcClaims {
    pub email: String,
    pub display_name: String,
    pub roles: Vec<String>,
}

fn http_client(allow_insecure: bool) -> Result<reqwest::Client, ApiError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        // ne jamais suivre les redirects du fournisseur (openidconnect l'exige)
        .redirect(Policy::none())
        .danger_accept_invalid_certs(allow_insecure)
        .build()
        .map_err(|e| ApiError::Internal(format!("client HTTP OIDC: {e}")))
}

async fn build_client(
    cfg: &OidcConfig,
    redirect_uri: &str,
    http: &reqwest::Client,
) -> Result<DiscoveredClient, ApiError> {
    let issuer = IssuerUrl::new(cfg.issuer_url.clone())
        .map_err(|e| ApiError::Validation(format!("issuer_url invalide: {e}")))?;
    let metadata = CoreProviderMetadata::discover_async(issuer, http)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "OIDC discovery");
            ApiError::BadGateway("fournisseur SSO injoignable ou metadata invalides".into())
        })?;
    let client_secret = cfg.client_secret.clone().unwrap_or_default();
    Ok(DiscoveredClient::from_provider_metadata(
        metadata,
        ClientId::new(cfg.client_id.clone()),
        if client_secret.is_empty() {
            None
        } else {
            Some(ClientSecret::new(client_secret))
        },
    )
    .set_redirect_uri(
        RedirectUrl::new(redirect_uri.to_string())
            .map_err(|e| ApiError::Validation(format!("redirect_uri invalide: {e}")))?,
    ))
}

/// Décode le payload (segment du milieu) d'un JWT compact, sans vérification
/// (la vérification a déjà été faite par openidconnect sur le token entier).
pub fn decode_jwt_payload(token: &str) -> Result<Value, ApiError> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| ApiError::Internal("id_token malformé".into()))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| ApiError::Internal("id_token malformé (base64)".into()))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::Internal("id_token malformé (JSON)".into()))
}

/// Démarre un flux OIDC : URL d'autorisation du fournisseur (state + nonce + PKCE).
pub async fn start(cfg: &OidcConfig, redirect_uri: &str) -> Result<OidcStart, ApiError> {
    let http = http_client(cfg.tls_allow_insecure)?;
    let client = build_client(cfg, redirect_uri, &http).await?;
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let mut req = client.authorize_url(
        CoreAuthenticationFlow::AuthorizationCode,
        CsrfToken::new_random,
        Nonce::new_random,
    );
    for s in cfg.scopes.split_whitespace() {
        req = req.add_scope(Scope::new(s.to_string()));
    }
    let (url, csrf, nonce) = req.set_pkce_challenge(pkce_challenge).url();
    Ok(OidcStart {
        authorization_url: url.to_string(),
        state: csrf.secret().clone(),
        nonce: nonce.secret().clone(),
        pkce_verifier: pkce_verifier.secret().clone(),
    })
}

/// Échange le code contre un id_token vérifié et en extrait les claims.
pub async fn exchange(
    cfg: &OidcConfig,
    redirect_uri: &str,
    code: &str,
    pkce_verifier: &str,
    nonce: &str,
) -> Result<OidcClaims, ApiError> {
    let http = http_client(cfg.tls_allow_insecure)?;
    let client = build_client(cfg, redirect_uri, &http).await?;
    let token_response = client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .map_err(|e| ApiError::Internal(format!("échange de code: {e}")))?
        .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_string()))
        .request_async(&http)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "OIDC token exchange");
            ApiError::Unauthorized
        })?;
    let id_token = token_response.id_token().ok_or(ApiError::Unauthorized)?;
    let expected_nonce = Nonce::new(nonce.to_string());
    let claims = id_token
        .claims(&client.id_token_verifier(), &expected_nonce)
        .map_err(|e| {
            tracing::warn!(error = %e, "OIDC id_token claims");
            ApiError::Unauthorized
        })?;
    // Claims métier lus dans le payload vérifié (chemins pointillés possibles).
    let payload = decode_jwt_payload(&id_token.to_string())?;
    let email = dotted_get(&payload, &cfg.email_claim)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| claims.email().map(|e| e.as_str().to_string()))
        .filter(|e| e.contains('@'))
        .ok_or(ApiError::Unauthorized)?;
    let display_name = dotted_get(&payload, &cfg.name_claim)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| email.clone());
    let roles = roles_from_claim(dotted_get(&payload, &cfg.roles_claim));
    Ok(OidcClaims {
        email,
        display_name,
        roles,
    })
}

/// Test de configuration (bouton « Tester ») : discovery du fournisseur.
pub async fn test_discovery(cfg: &OidcConfig) -> Result<String, ApiError> {
    let http = http_client(cfg.tls_allow_insecure)?;
    let issuer = IssuerUrl::new(cfg.issuer_url.clone())
        .map_err(|e| ApiError::Device(format!("issuer_url invalide: {e}")))?;
    let md = CoreProviderMetadata::discover_async(issuer, &http)
        .await
        .map_err(|e| ApiError::Device(format!("discovery: {e}")))?;
    Ok(format!(
        "découverte OK — issuer {}, endpoint {}",
        md.issuer().as_str(),
        md.authorization_endpoint().as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Form, Json, Router,
        routing::{get, post},
    };
    use jsonwebtoken::{Algorithm, EncodingKey, Header};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn payload_jwt_decode() {
        // header.payload.signature en base64url sans padding
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"u1","roles":["admin"]}"#);
        let token = format!("aaa.{payload}.ccc");
        let v = decode_jwt_payload(&token).unwrap();
        assert_eq!(v["sub"], json!("u1"));
        assert_eq!(v["roles"], json!(["admin"]));
    }

    /// Mock OIDC réel (HS256, vérifié avec le client_secret) : discovery + JWKS
    /// + token. Le « code » reçu contient le nonce à réinjecter dans l'id_token.
    #[tokio::test]
    async fn oidc_flow_complet_contre_mock() {
        let client_id = "supellex".to_string();
        let client_secret = "mock-secret-0123456789abcdef".to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let issuer = format!("http://127.0.0.1:{port}");
        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get({
                    let iss = issuer.clone();
                    move || {
                        let iss = iss.clone();
                        async move {
                            Json(json!({
                                "issuer": iss,
                                "authorization_endpoint": format!("{iss}/authorize"),
                                "token_endpoint": format!("{iss}/token"),
                                "jwks_uri": format!("{iss}/jwks"),
                                "response_types_supported": ["code"],
                                "subject_types_supported": ["public"],
                                "id_token_signing_alg_values_supported": ["HS256", "RS256"],
                                "scopes_supported": ["openid", "email", "profile"],
                                "token_endpoint_auth_methods_supported": ["client_secret_basic"],
                                "grant_types_supported": ["authorization_code"]
                            }))
                        }
                    }
                }),
            )
            .route(
                "/jwks",
                get({
                    let cs = client_secret.clone();
                    move || {
                        let k = URL_SAFE_NO_PAD.encode(cs.as_bytes());
                        async move {
                            Json(json!({"keys": [{"kty": "oct", "alg": "HS256", "use": "sig", "k": k, "kid": "hs256"}]}))
                        }
                    }
                }),
            )
            .route(
                "/token",
                post({
                    let cs = client_secret.clone();
                    let cid = client_id.clone();
                    let iss = issuer.clone();
                    move |Form(form): Form<HashMap<String, String>>| {
                        let cs = cs.clone();
                        let cid = cid.clone();
                        let iss = iss.clone();
                        async move {
                            let nonce = form.get("code").cloned().unwrap_or_default();
                            let now = chrono::Utc::now().timestamp();
                            let claims = json!({
                                "iss": iss,
                                "sub": "user-1",
                                "aud": cid,
                                "exp": now + 300,
                                "iat": now,
                                "nonce": nonce,
                                "email": "alice@example.com",
                                "name": "Alice Martin",
                                "roles": ["supellex-admin", "equipe-it"]
                            });
                            let token = jsonwebtoken::encode(
                                &Header::new(Algorithm::HS256),
                                &claims,
                                &EncodingKey::from_secret(cs.as_bytes()).expect("clé HS256"),
                            )
                            .unwrap();
                            Json(json!({
                                "access_token": "at-mock",
                                "token_type": "Bearer",
                                "expires_in": 3600,
                                "id_token": token
                            }))
                        }
                    }
                }),
            );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let redirect = "http://localhost:3000/api/auth/oidc/callback".to_string();
        let cfg = OidcConfig {
            issuer_url: issuer.clone(),
            client_id,
            client_secret: Some(client_secret.clone()),
            redirect_uri: Some(redirect.clone()),
            ..Default::default()
        };
        let started = start(&cfg, &redirect).await.unwrap();
        assert!(
            started
                .authorization_url
                .starts_with(&format!("{issuer}/authorize"))
        );
        assert!(started.authorization_url.contains("code_challenge="));
        assert!(
            started
                .authorization_url
                .contains("code_challenge_method=S256")
        );

        // Le mock reçoit le nonce comme « code ».
        let claims = exchange(
            &cfg,
            &redirect,
            &started.nonce,
            &started.pkce_verifier,
            &started.nonce,
        )
        .await
        .unwrap();
        assert_eq!(claims.email, "alice@example.com");
        assert_eq!(claims.display_name, "Alice Martin");
        assert_eq!(claims.roles, vec!["supellex-admin", "equipe-it"]);

        // Nonce incorrect : rejeté.
        let bad = exchange(
            &cfg,
            &redirect,
            &started.nonce,
            &started.pkce_verifier,
            "mauvais-nonce",
        )
        .await;
        assert!(bad.is_err());

        // Mauvais client_secret : la vérification de signature échoue.
        let mut cfg2 = cfg.clone();
        cfg2.client_secret = Some("mauvais-secret".to_string());
        let bad2 = exchange(
            &cfg2,
            &redirect,
            &started.nonce,
            &started.pkce_verifier,
            &started.nonce,
        )
        .await;
        assert!(bad2.is_err());
    }
}
