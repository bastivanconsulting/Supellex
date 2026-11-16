//! Authentification LDAP : recherche de l'utilisateur (bind compte de
//! service ou bind direct via DN template) puis vérification du mot de passe
//! par un bind sur le DN de l'utilisateur. Aucune crypto maison, timeouts
//! bornés, mot de passe jamais logué. TLS via rustls (ldaps:// ou StartTLS).

use std::time::Duration;

use ldap3::{LdapConnAsync, LdapConnSettings, Scope, SearchEntry};

use crate::auth_providers::LdapConfig;
use crate::error::ApiError;

/// Résultat d'une authentification LDAP réussie.
pub struct LdapUserInfo {
    pub email: String,
    pub display_name: String,
    pub groups: Vec<String>,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const OP_TIMEOUT: Duration = Duration::from_secs(15);

/// Échappement d'une valeur de filtre LDAP (RFC 4515 §3).
pub fn escape_filter_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '*' => out.push_str("\\2a"),
            '(' => out.push_str("\\28"),
            ')' => out.push_str("\\29"),
            '\\' => out.push_str("\\5c"),
            '\0' => out.push_str("\\00"),
            _ => out.push(c),
        }
    }
    out
}

/// Échappement d'une valeur de DN (RFC 4514 §2.4).
pub fn escape_dn_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for (i, c) in s.chars().enumerate() {
        match c {
            ',' | '+' | '"' | '\\' | '<' | '>' | ';' => {
                out.push('\\');
                out.push(c);
            }
            '#' if i == 0 => out.push_str("\\#"),
            ' ' if i == 0 || i == s.chars().count() - 1 => out.push_str("\\ "),
            _ => out.push(c),
        }
    }
    out
}

/// Substitue {login} dans le template de filtre (valeur échappée RFC 4515).
pub fn build_user_filter(template: &str, login: &str) -> String {
    template.replace("{login}", &escape_filter_value(login))
}

fn map_err(_: ldap3::LdapError) -> ApiError {
    // Message générique : pas de détail serveur LDAP exposé au client.
    ApiError::BadCredentials
}

/// Authentifie `login` / `password` auprès de l'annuaire décrit par `cfg`.
pub async fn authenticate(
    cfg: &LdapConfig,
    login: &str,
    password: &str,
) -> Result<LdapUserInfo, ApiError> {
    if login.trim().is_empty() || password.is_empty() {
        return Err(ApiError::BadCredentials);
    }
    tokio::time::timeout(OP_TIMEOUT, authenticate_inner(cfg, login, password))
        .await
        .map_err(|_| ApiError::BadCredentials)?
}

async fn authenticate_inner(
    cfg: &LdapConfig,
    login: &str,
    password: &str,
) -> Result<LdapUserInfo, ApiError> {
    let mut settings = LdapConnSettings::new()
        .set_conn_timeout(CONNECT_TIMEOUT)
        .set_starttls(cfg.starttls)
        .set_no_tls_verify(cfg.tls_allow_insecure);
    // ldaps:// : TLS implicite géré par ldap3 ; starttls sinon si demandé.
    if cfg.url.starts_with("ldap://") && !cfg.starttls {
        // clair volontaire (ex. localhost) : rien à faire
    }
    let _ = &mut settings;

    let (conn, mut ldap) = LdapConnAsync::with_settings(settings, &cfg.url)
        .await
        // Login : même réponse qu'un mauvais mot de passe (ne pas divulguer
        // l'état de l'infrastructure annuaire) ; le bouton « Tester » admin
        // donne le détail.
        .map_err(|_| ApiError::BadCredentials)?;
    tokio::spawn(async move {
        let _ = conn.drive().await;
    });

    let entry: SearchEntry = if let Some(template) = &cfg.user_dn_template {
        // Bind direct : le DN est dérivé du login, le bind vaut vérification.
        let dn = template.replace("{login}", &escape_dn_value(login));
        let res = ldap.simple_bind(&dn, password).await.map_err(map_err)?;
        if res.rc != 0 {
            return Err(ApiError::BadCredentials);
        }
        // Relecture des attributs de l'utilisateur pour e-mail / groupes.
        let attrs = vec![
            cfg.attr_email.as_str(),
            cfg.attr_display_name.as_str(),
            cfg.attr_groups.as_str(),
            "cn",
        ];
        let (entries, _) = ldap
            .search(&dn, Scope::Base, "(objectClass=*)", attrs)
            .await
            .map_err(map_err)?
            .success()
            .map_err(map_err)?;
        entries
            .into_iter()
            .next()
            .map(SearchEntry::construct)
            .ok_or(ApiError::BadCredentials)?
    } else {
        // Bind compte de service puis recherche, puis bind utilisateur.
        let bind_dn = cfg
            .bind_dn
            .clone()
            .ok_or(ApiError::Internal("config ldap incomplète".into()))?;
        let bind_pw = cfg.bind_password.clone().unwrap_or_default();
        let res = ldap
            .simple_bind(&bind_dn, &bind_pw)
            .await
            .map_err(map_err)?;
        if res.rc != 0 {
            return Err(ApiError::Internal("bind service refusé".into()));
        }
        let filter = build_user_filter(&cfg.user_filter, login);
        let attrs = vec![
            cfg.attr_email.as_str(),
            cfg.attr_display_name.as_str(),
            cfg.attr_groups.as_str(),
            "cn",
            cfg.login_attribute.as_str(),
        ];
        let (entries, _) = ldap
            .search(&cfg.base_dn, Scope::Subtree, &filter, attrs)
            .await
            .map_err(map_err)?
            .success()
            .map_err(map_err)?;
        let entry = entries
            .into_iter()
            .next()
            .map(SearchEntry::construct)
            .ok_or(ApiError::BadCredentials)?;
        // Vérification du mot de passe : bind sur le DN trouvé.
        let res = ldap
            .simple_bind(&entry.dn, password)
            .await
            .map_err(map_err)?;
        if res.rc != 0 {
            return Err(ApiError::BadCredentials);
        }
        entry
    };

    let _ = ldap.unbind().await;

    let attr =
        |name: &str| -> Option<String> { entry.attrs.get(name).and_then(|v| v.first()).cloned() };
    let email = attr(&cfg.attr_email)
        .filter(|e| e.contains('@'))
        .or_else(|| login.contains('@').then(|| login.to_string()))
        .ok_or_else(|| ApiError::Internal("compte LDAP sans attribut e-mail".into()))?;
    let display_name = attr(&cfg.attr_display_name)
        .or_else(|| attr("cn"))
        .unwrap_or_else(|| login.to_string());
    let groups = entry
        .attrs
        .get(&cfg.attr_groups)
        .cloned()
        .unwrap_or_default();
    Ok(LdapUserInfo {
        email,
        display_name,
        groups,
    })
}

/// Test de configuration (bouton « Tester ») : connexion + bind service.
pub async fn test_connection(cfg: &LdapConfig) -> Result<String, ApiError> {
    tokio::time::timeout(OP_TIMEOUT, async {
        let settings = LdapConnSettings::new()
            .set_conn_timeout(CONNECT_TIMEOUT)
            .set_starttls(cfg.starttls)
            .set_no_tls_verify(cfg.tls_allow_insecure);
        let (conn, mut ldap) = LdapConnAsync::with_settings(settings, &cfg.url)
            .await
            .map_err(|e| ApiError::Device(format!("connexion refusée: {e}")))?;
        tokio::spawn(async move {
            let _ = conn.drive().await;
        });
        let (dn, pw) = match (&cfg.bind_dn, &cfg.bind_password) {
            (Some(dn), Some(pw)) => (dn.clone(), pw.clone()),
            _ => ("".to_string(), String::new()), // bind anonyme
        };
        let res = ldap
            .simple_bind(&dn, &pw)
            .await
            .map_err(|e| ApiError::Device(format!("bind refusé: {e}")))?;
        let _ = ldap.unbind().await;
        if res.rc != 0 {
            return Err(ApiError::Device(format!("bind refusé (rc={})", res.rc)));
        }
        Ok("connexion et bind OK".to_string())
    })
    .await
    .map_err(|_| ApiError::Device("délai dépassé (15 s)".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_filtre_rfc4515() {
        assert_eq!(escape_filter_value("a*b"), "a\\2ab");
        assert_eq!(escape_filter_value("(x)"), "\\28x\\29");
        assert_eq!(escape_filter_value("back\\slash"), "back\\5cslash");
        assert_eq!(escape_filter_value("nul\0ici"), "nul\\00ici");
        assert_eq!(escape_filter_value(" użytkownik "), " użytkownik ");
    }

    #[test]
    fn escape_dn_rfc4514() {
        assert_eq!(escape_dn_value("a,b"), "a\\,b");
        assert_eq!(escape_dn_value("#hash"), "\\#hash");
        assert_eq!(escape_dn_value(" leading"), "\\ leading");
        assert_eq!(escape_dn_value("trailing "), "trailing\\ ");
        assert_eq!(escape_dn_value("uid=unix"), "uid=unix");
    }

    #[test]
    fn filtre_substitution_echappee() {
        let f = build_user_filter("(&(objectClass=person)(uid={login}))", "a*(b)");
        assert_eq!(f, "(&(objectClass=person)(uid=a\\2a\\28b\\29))");
    }
}
