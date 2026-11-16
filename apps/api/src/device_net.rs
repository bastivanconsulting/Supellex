//! Connexion aux appareils réseau : SSH (russh, mot de passe ou clé privée)
//! et API RouterOS MikroTik (protocole natif TCP 8728, implémenté ici).
//! Aucune dépendance externe pour l'API : le protocole est
//! mot-à-mot avec préfixe de longueur.

use std::sync::Arc;
use std::time::Duration;

use russh::ChannelMsg;
use russh::client::{Config, Handle, Handler};
use russh::keys::{HashAlg, PublicKey, PublicKeyOrCertificate};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::error::ApiError;

pub enum DeviceAuth<'a> {
    Password(&'a str),
    Key(&'a str),
}

pub struct SshResult {
    pub output: String,
    pub exit_status: u32,
    /// Empreinte SHA256 de la clé publique du serveur (TOFU).
    pub host_key_fp: String,
}

fn fp(key: &PublicKey) -> String {
    format!("{}", key.fingerprint(HashAlg::Sha256))
}

struct ClientHandler {
    expected_fp: Option<String>,
    seen_fp: Arc<Mutex<Option<String>>>,
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // Seules les clés publiques simples sont acceptées (pas les certificats).
        let key = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key,
            PublicKeyOrCertificate::Certificate(_) => return Ok(false),
        };
        let presented = fp(key);
        *self.seen_fp.lock().await = Some(presented.clone());
        // TOFU : première connexion -> on épingle ; ensuite on vérifie.
        Ok(self.expected_fp.as_deref().is_none_or(|e| e == presented))
    }
}

/// Exécute `command` sur l'appareil en SSH. `expected_fp` = empreinte épinglée
/// (None au premier contact). Renvoie la sortie + le code de retour.
pub async fn ssh_exec(
    host: &str,
    port: u16,
    username: &str,
    auth: DeviceAuth<'_>,
    expected_fp: Option<&str>,
    command: &str,
) -> Result<SshResult, ApiError> {
    let seen_fp = Arc::new(Mutex::new(None));
    let handler = ClientHandler {
        expected_fp: expected_fp.map(|s| s.to_string()),
        seen_fp: seen_fp.clone(),
    };
    let config = Config {
        inactivity_timeout: Some(Duration::from_secs(15)),
        keepalive_interval: Some(Duration::from_secs(10)),
        ..Default::default()
    };

    let connect = russh::client::connect(Arc::new(config), (host, port), handler);
    let mut handle: Handle<ClientHandler> = tokio::time::timeout(Duration::from_secs(15), connect)
        .await
        .map_err(|_| ApiError::Device("délai de connexion SSH dépassé".into()))?
        .map_err(|e| ApiError::Device(format!("connexion SSH échouée: {e}")))?;

    let auth_result = match auth {
        DeviceAuth::Password(p) => handle.authenticate_password(username, p).await,
        DeviceAuth::Key(pem) => {
            let key = russh::keys::decode_secret_key(pem, None).map_err(|e| {
                ApiError::Device(format!("clé SSH illisible (ed25519/ECDSA attendue): {e}"))
            })?;
            let with_alg = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None);
            handle.authenticate_publickey(username, with_alg).await
        }
    }
    .map_err(|e| ApiError::Device(format!("authentification SSH échouée: {e}")))?;
    if !auth_result.success() {
        return Err(ApiError::Device(
            "authentification SSH refusée (identifiants invalides)".into(),
        ));
    }

    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| ApiError::Device(format!("ouverture de canal SSH: {e}")))?;
    channel
        .exec(true, command)
        .await
        .map_err(|e| ApiError::Device(format!("exécution distante: {e}")))?;

    let mut output = Vec::new();
    let mut exit_status = 0u32;
    let read = async {
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => output.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, .. } => output.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status: s } => exit_status = s,
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(60), read)
        .await
        .map_err(|_| ApiError::Device("délai d'exécution dépassé".into()))?;

    let host_key_fp = seen_fp.lock().await.clone().unwrap_or_default();
    Ok(SshResult {
        output: String::from_utf8_lossy(&output).to_string(),
        exit_status,
        host_key_fp,
    })
}

// ---------- API RouterOS (MikroTik), port 8728 ----------

pub struct ApiReply {
    pub traps: Vec<String>,
    pub data: Vec<String>,
}

impl ApiReply {
    pub fn ok(&self) -> bool {
        self.traps.is_empty()
    }
}

fn encode_len(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else if len < 0x4000 {
        vec![0x80 | (len >> 8) as u8, (len & 0xff) as u8]
    } else if len < 0x200000 {
        vec![
            0xC0 | (len >> 16) as u8,
            (len >> 8) as u8,
            (len & 0xff) as u8,
        ]
    } else if len < 0x10000000 {
        vec![
            0xE0 | (len >> 24) as u8,
            (len >> 16) as u8,
            (len >> 8) as u8,
            (len & 0xff) as u8,
        ]
    } else {
        vec![
            0xF0,
            (len >> 24) as u8,
            (len >> 16) as u8,
            (len >> 8) as u8,
            (len & 0xff) as u8,
        ]
    }
}

async fn send_sentence(stream: &mut TcpStream, words: &[String]) -> Result<(), ApiError> {
    let mut buf = Vec::new();
    for w in words {
        buf.extend_from_slice(&encode_len(w.len()));
        buf.extend_from_slice(w.as_bytes());
    }
    buf.push(0); // fin de phrase
    stream
        .write_all(&buf)
        .await
        .map_err(|e| ApiError::Device(format!("API RouterOS, envoi: {e}")))?;
    Ok(())
}

async fn read_word(stream: &mut TcpStream) -> Result<Option<String>, ApiError> {
    let mut first = [0u8; 1];
    stream
        .read_exact(&mut first)
        .await
        .map_err(|e| ApiError::Device(format!("API RouterOS, lecture: {e}")))?;
    let len = if first[0] & 0x80 == 0 {
        first[0] as usize
    } else if first[0] & 0xC0 == 0x80 {
        let mut b = [0u8; 1];
        stream
            .read_exact(&mut b)
            .await
            .map_err(|e| ApiError::Device(format!("API RouterOS: {e}")))?;
        ((first[0] as usize & 0x3f) << 8) | b[0] as usize
    } else if first[0] & 0xE0 == 0xC0 {
        let mut b = [0u8; 2];
        stream
            .read_exact(&mut b)
            .await
            .map_err(|e| ApiError::Device(format!("API RouterOS: {e}")))?;
        ((first[0] as usize & 0x1f) << 16) | (b[0] as usize) << 8 | b[1] as usize
    } else {
        let mut b = [0u8; 3];
        stream
            .read_exact(&mut b)
            .await
            .map_err(|e| ApiError::Device(format!("API RouterOS: {e}")))?;
        ((first[0] as usize & 0x0f) << 24)
            | (b[0] as usize) << 16
            | (b[1] as usize) << 8
            | b[2] as usize
    };
    if len == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| ApiError::Device(format!("API RouterOS: {e}")))?;
    String::from_utf8(buf)
        .map_err(|_| ApiError::Device("API RouterOS: mot non UTF-8".into()))
        .map(Some)
}

/// Lit une phrase de réponse complète (!re / !done / !trap / !fatal).
async fn read_reply_sentence(stream: &mut TcpStream) -> Result<Vec<String>, ApiError> {
    let mut words = Vec::new();
    while let Some(w) = read_word(stream).await? {
        words.push(w);
    }
    Ok(words)
}

/// Exécute des commandes sur l'API RouterOS. Chaque commande est une liste de
/// mots (ex. ["/interface/vlan/add", "=name=vlan10", "=vlan-id=10"]).
/// Login moderne (RouterOS >= 6.43) : /login =name= =password=.
pub async fn routeros_api(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    commands: &[Vec<String>],
) -> Result<Vec<ApiReply>, ApiError> {
    let connect = TcpStream::connect((host, port));
    let mut stream = tokio::time::timeout(Duration::from_secs(15), connect)
        .await
        .map_err(|_| ApiError::Device("délai de connexion API dépassé".into()))?
        .map_err(|e| ApiError::Device(format!("connexion API échouée: {e}")))?;

    let run = async {
        // Login
        send_sentence(
            &mut stream,
            &[
                "/login".into(),
                format!("=name={user}"),
                format!("=password={password}"),
            ],
        )
        .await?;
        let login = read_reply_sentence(&mut stream).await?;
        if login.iter().any(|w| w == "!trap") || login.iter().any(|w| w == "!fatal") {
            let msg = login
                .iter()
                .find(|w| w.starts_with("=message="))
                .cloned()
                .unwrap_or_default();
            return Err(ApiError::Device(format!("login API refusé: {msg}")));
        }
        if login.iter().any(|w| w == "!re") {
            return Err(ApiError::Device(
                "RouterOS < 6.43 (login par challenge MD5) non supporté, >= 6.43 requis".into(),
            ));
        }
        let mut replies = Vec::new();
        for cmd in commands {
            send_sentence(&mut stream, cmd).await?;
            let mut traps = Vec::new();
            let mut data = Vec::new();
            loop {
                let words = read_reply_sentence(&mut stream).await?;
                if words.is_empty() {
                    continue;
                }
                match words[0].as_str() {
                    "!re" => data.extend(words[1..].iter().cloned()),
                    "!trap" | "!fatal" => {
                        traps.push(
                            words
                                .iter()
                                .find(|w| w.starts_with("=message="))
                                .cloned()
                                .unwrap_or_else(|| words.join(" ")),
                        );
                    }
                    "!done" => {
                        replies.push(ApiReply { traps, data });
                        break;
                    }
                    _ => {}
                }
            }
        }
        Ok(replies)
    };
    tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .map_err(|_| ApiError::Device("délai API dépassé".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodage_longueurs() {
        assert_eq!(encode_len(5), vec![5]);
        assert_eq!(encode_len(0x7f), vec![0x7f]);
        assert_eq!(encode_len(0x80), vec![0x80, 0x80]);
        assert_eq!(encode_len(0x3fff), vec![0xbf, 0xff]);
        assert_eq!(encode_len(0x4000), vec![0xc0, 0x40, 0x00]);
    }
}
