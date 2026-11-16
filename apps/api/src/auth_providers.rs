//! Sources d'authentification externes configurables par un admin :
//! annuaire LDAP et fournisseurs SSO (OIDC). La source « local » (table
//! users, argon2id) existe toujours et n'est pas stockée ici.
//!
//! Les secrets des fournisseurs (mot de passe du compte de service LDAP,
//! client_secret OIDC) sont chiffrés au repos AES-256-GCM dans le JSONB de
//! config avec le préfixe "enc:" (AAD distinct des secrets devices) et ne
//! sont jamais renvoyés par l'API (masqués à la lecture).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::Role;
use crate::crypto::SecretCipher;
use crate::error::ApiError;

pub const ENC_PREFIX: &str = "enc:";
pub const MASK: &str = "••••••";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Ldap,
    Oidc,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Ldap => "ldap",
            ProviderKind::Oidc => "oidc",
        }
    }
    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "ldap" => Some(ProviderKind::Ldap),
            "oidc" => Some(ProviderKind::Oidc),
            _ => None,
        }
    }
    /// Champs du JSONB de config considérés comme secrets (chiffrés au repos).
    pub fn secret_fields(&self) -> &'static [&'static str] {
        match self {
            ProviderKind::Ldap => &["bind_password"],
            ProviderKind::Oidc => &["client_secret"],
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct LdapConfig {
    /// ldap:// ou ldaps:// (TLS implicite), ex. ldaps://ldap.example.com:636
    pub url: String,
    pub base_dn: String,
    /// Compte de service pour la recherche (bind avant search). Si absent,
    /// `user_dn_template` est requis (bind direct).
    pub bind_dn: Option<String>,
    /// Secret chiffré au repos ("enc:..."), déchiffré en mémoire.
    pub bind_password: Option<String>,
    /// Filtre de recherche, {login} est échappé RFC 4515 avant substitution.
    pub user_filter: String,
    /// Alternative au compte de service : DN direct, ex. "uid={login},ou=people,dc=…".
    pub user_dn_template: Option<String>,
    /// Attribut saisi par l'utilisateur (ex. "uid", "mail") ; exposé publiquement
    /// comme label du champ de login.
    pub login_attribute: String,
    pub attr_email: String,
    pub attr_display_name: String,
    pub attr_groups: String,
    /// StartTLS sur un port ldap:// (3890/389) au lieu de ldaps://.
    pub starttls: bool,
    /// Désactive la vérification du certificat serveur (déconseillé, test uniquement).
    pub tls_allow_insecure: bool,
    /// "valeur LDAP (ex. DN memberOf)" -> rôle supellex.
    pub role_map: HashMap<String, String>,
    pub default_role: Role,
    /// Crée le compte local à la première connexion externe (JIT).
    pub jit_provision: bool,
    /// Recalcule le rôle depuis role_map à chaque connexion (comptes existants).
    pub sync_roles: bool,
}

impl Default for LdapConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            base_dn: String::new(),
            bind_dn: None,
            bind_password: None,
            user_filter: "(&(objectClass=person)(uid={login}))".into(),
            user_dn_template: None,
            login_attribute: "uid".into(),
            attr_email: "mail".into(),
            attr_display_name: "displayName".into(),
            attr_groups: "memberOf".into(),
            starttls: false,
            tls_allow_insecure: false,
            role_map: HashMap::new(),
            default_role: Role::Viewer,
            jit_provision: true,
            sync_roles: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct OidcConfig {
    /// URL de l'émetteur (discovery /.well-known/openid-configuration), ex.
    /// https://sso.example.com/realms/supellex
    pub issuer_url: String,
    pub client_id: String,
    /// Secret chiffré au repos ("enc:...").
    pub client_secret: Option<String>,
    /// redirect_uri enregistré chez le fournisseur. Par défaut :
    /// <PUBLIC_BASE_URL>/api/auth/oidc/callback
    pub redirect_uri: Option<String>,
    pub scopes: String,
    /// Claim e-mail (chemin pointillé supporté, ex. "realm_access.email").
    pub email_claim: String,
    pub name_claim: String,
    /// Claim de rôles (tableau de chaînes ou chaîne, ex. "roles").
    pub roles_claim: String,
    pub role_map: HashMap<String, String>,
    pub default_role: Role,
    pub jit_provision: bool,
    pub sync_roles: bool,
    pub tls_allow_insecure: bool,
}

impl Default for OidcConfig {
    fn default() -> Self {
        Self {
            issuer_url: String::new(),
            client_id: String::new(),
            client_secret: None,
            redirect_uri: None,
            scopes: "openid email profile".into(),
            email_claim: "email".into(),
            name_claim: "name".into(),
            roles_claim: "roles".into(),
            role_map: HashMap::new(),
            default_role: Role::Viewer,
            jit_provision: true,
            sync_roles: true,
            tls_allow_insecure: false,
        }
    }
}

/// Valide un JSONB de config pour un kind donné (parse + champs requis).
pub fn parse_config(kind: ProviderKind, config: &Value) -> Result<(), ApiError> {
    let bad = |m: &str| ApiError::Validation(format!("config {kind:?}: {m}"));
    match kind {
        ProviderKind::Ldap => {
            let c: LdapConfig =
                serde_json::from_value(config.clone()).map_err(|e| bad(&e.to_string()))?;
            if !(c.url.starts_with("ldap://") || c.url.starts_with("ldaps://")) {
                return Err(bad("url doit commencer par ldap:// ou ldaps://"));
            }
            if c.base_dn.trim().is_empty() {
                return Err(bad("base_dn requis"));
            }
            if c.bind_dn.is_none() && c.user_dn_template.is_none() {
                return Err(bad(
                    "bind_dn (compte de service) ou user_dn_template requis",
                ));
            }
            if c.user_dn_template.is_none() && !c.user_filter.contains("{login}") {
                return Err(bad("user_filter doit contenir le placeholder {login}"));
            }
            validate_role_map(&c.role_map)?;
        }
        ProviderKind::Oidc => {
            let c: OidcConfig =
                serde_json::from_value(config.clone()).map_err(|e| bad(&e.to_string()))?;
            if !c.issuer_url.starts_with("https://") && !c.issuer_url.starts_with("http://") {
                return Err(bad("issuer_url doit être une URL http(s)"));
            }
            if c.client_id.trim().is_empty() {
                return Err(bad("client_id requis"));
            }
            if let Some(r) = &c.redirect_uri
                && !r.starts_with("https://")
                && !r.starts_with("http://")
            {
                return Err(bad("redirect_uri doit être une URL http(s)"));
            }
            validate_role_map(&c.role_map)?;
        }
    }
    Ok(())
}

fn validate_role_map(map: &HashMap<String, String>) -> Result<(), ApiError> {
    for (k, v) in map {
        if k.trim().is_empty() {
            return Err(ApiError::Validation("role_map: clé vide".into()));
        }
        if Role::from_db(v).is_none() {
            return Err(ApiError::Validation(format!(
                "role_map: rôle inconnu « {v} » (attendu: viewer, operator, admin)"
            )));
        }
    }
    Ok(())
}

/// Chiffre les champs secrets d'un JSONB de config (préfixe "enc:"). Idempotent.
pub fn encrypt_secrets(
    cipher: &SecretCipher,
    kind: ProviderKind,
    config: &mut Value,
) -> Result<(), ApiError> {
    if let Some(obj) = config.as_object_mut() {
        for field in kind.secret_fields() {
            if let Some(v) = obj.get_mut(*field)
                && let Some(s) = v.as_str()
                && !s.is_empty()
                && !s.starts_with(ENC_PREFIX)
                && s != MASK
            {
                *v = Value::String(format!("{ENC_PREFIX}{}", cipher.encrypt(s)?));
            }
        }
    }
    Ok(())
}

/// Déchiffre les champs secrets d'un JSONB de config (en place, copie).
pub fn decrypt_secrets(
    cipher: &SecretCipher,
    kind: ProviderKind,
    config: &Value,
) -> Result<Value, ApiError> {
    let mut out = config.clone();
    if let Some(obj) = out.as_object_mut() {
        for field in kind.secret_fields() {
            if let Some(v) = obj.get_mut(*field)
                && let Some(s) = v.as_str()
                && let Some(enc) = s.strip_prefix(ENC_PREFIX)
            {
                *v = Value::String(cipher.decrypt(enc)?);
            }
        }
    }
    Ok(out)
}

/// Masque les champs secrets pour une réponse API (jamais de valeur réelle).
pub fn mask_secrets(kind: ProviderKind, config: &Value) -> Value {
    let mut out = config.clone();
    if let Some(obj) = out.as_object_mut() {
        for field in kind.secret_fields() {
            if obj.get(*field).is_some() {
                obj.insert((*field).to_string(), Value::String(MASK.to_string()));
            }
        }
    }
    out
}

/// Rôle le plus élevé mappé parmi les valeurs LDAP/claims fournies.
/// None si aucune ne correspond à `role_map`.
pub fn mapped_role(role_map: &HashMap<String, String>, matched: &[String]) -> Option<Role> {
    let mut best: Option<Role> = None;
    for m in matched {
        if let Some(r) = role_map.get(m).and_then(|v| Role::from_db(v)) {
            best = Some(match best {
                Some(b) if b >= r => b,
                _ => r,
            });
        }
    }
    best
}

/// Lecture d'un claim via un chemin pointillé ("realm_access.roles").
pub fn dotted_get<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = value;
    for part in path.split('.') {
        cur = cur.get(part)?;
    }
    Some(cur)
}

/// Normalise un claim de rôles en liste de chaînes (chaîne seule ou tableau).
pub fn roles_from_claim(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn role_map_max_role_gagne() {
        let mut map = HashMap::new();
        map.insert("cn=ops,dc=x".to_string(), "operator".to_string());
        map.insert("cn=admins,dc=x".to_string(), "admin".to_string());
        let matched = vec!["cn=ops,dc=x".to_string(), "cn=admins,dc=x".to_string()];
        assert_eq!(mapped_role(&map, &matched), Some(Role::Admin));
        // aucun match : None (l'appelant garde le rôle courant ou le défaut)
        assert_eq!(mapped_role(&map, &["cn=autre".to_string()]), None);
    }

    #[test]
    fn dotted_get_chemin_pointille() {
        let v = json!({"realm_access": {"roles": ["a", "b"]}, "email": "x@y.z"});
        assert_eq!(
            dotted_get(&v, "realm_access.roles"),
            Some(&json!(["a", "b"]))
        );
        assert_eq!(dotted_get(&v, "email"), Some(&json!("x@y.z")));
        assert_eq!(dotted_get(&v, "a.b.c"), None);
    }

    #[test]
    fn roles_claim_string_ou_tableau() {
        assert_eq!(roles_from_claim(Some(&json!("admin"))), vec!["admin"]);
        assert_eq!(
            roles_from_claim(Some(&json!(["ops", "admin"]))),
            vec!["ops", "admin"]
        );
        assert!(roles_from_claim(Some(&json!(42))).is_empty());
        assert!(roles_from_claim(None).is_empty());
    }

    #[test]
    fn secrets_chiffres_puis_masques() {
        let cipher = SecretCipher::with_aad(&[3u8; 32], crate::crypto::AUTH_AAD);
        let mut cfg = json!({"url": "ldaps://x", "bind_password": "secret-LDAP"});
        encrypt_secrets(&cipher, ProviderKind::Ldap, &mut cfg).unwrap();
        let stored = cfg.clone();
        let s = stored["bind_password"].as_str().unwrap();
        assert!(s.starts_with(ENC_PREFIX));
        assert!(!s.contains("secret-LDAP"));
        // idempotent
        let mut again = stored.clone();
        encrypt_secrets(&cipher, ProviderKind::Ldap, &mut again).unwrap();
        assert_eq!(again, stored);
        // déchiffrement roundtrip
        let clear = decrypt_secrets(&cipher, ProviderKind::Ldap, &stored).unwrap();
        assert_eq!(clear["bind_password"], json!("secret-LDAP"));
        // masque
        let masked = mask_secrets(ProviderKind::Ldap, &stored);
        assert_eq!(masked["bind_password"], json!(MASK));
        // mauvaise AAD (celle des devices) : échec
        let dev = SecretCipher::new(&[3u8; 32]);
        assert!(decrypt_secrets(&dev, ProviderKind::Ldap, &stored).is_err());
    }

    #[test]
    fn config_ldap_valide_et_invalide() {
        let ok = json!({
            "url": "ldaps://ldap.example.com:636",
            "base_dn": "dc=example,dc=com",
            "bind_dn": "cn=svc,dc=example,dc=com",
            "bind_password": "enc:xxxx",
            "role_map": {"cn=admins,dc=example,dc=com": "admin"}
        });
        assert!(parse_config(ProviderKind::Ldap, &ok).is_ok());
        let ko_url = json!({"url": "http://x", "base_dn": "dc=x", "bind_dn": "cn=s"});
        assert!(parse_config(ProviderKind::Ldap, &ko_url).is_err());
        let ko_role = json!({
            "url": "ldaps://x", "base_dn": "dc=x", "bind_dn": "cn=s",
            "role_map": {"g": "superuser"}
        });
        assert!(parse_config(ProviderKind::Ldap, &ko_role).is_err());
        let ko_filter = json!({"url": "ldaps://x", "base_dn": "dc=x", "bind_dn": "cn=s", "user_filter": "(uid=x)"});
        assert!(parse_config(ProviderKind::Ldap, &ko_filter).is_err());
    }

    #[test]
    fn config_oidc_valide_et_invalide() {
        let ok = json!({"issuer_url": "https://sso.example.com/realms/x", "client_id": "supellex"});
        assert!(parse_config(ProviderKind::Oidc, &ok).is_ok());
        let ko = json!({"issuer_url": "https://sso.example.com", "client_id": " "});
        assert!(parse_config(ProviderKind::Oidc, &ko).is_err());
    }
}
