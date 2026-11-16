//! Chiffrement au repos des secrets des appareils réseau (AES-256-GCM,
//! algorithme standard, jamais de crypto maison). Format stocké :
//! base64( nonce 12 octets || chiffré || tag 16 octets ), AAD fixe.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand_core::{OsRng, RngCore};

use crate::error::ApiError;

const DEVICE_AAD: &[u8] = b"supellex:device-secret:v1";
/// AAD distinct pour les secrets des sources d'auth (bind LDAP, client OIDC).
pub const AUTH_AAD: &[u8] = b"supellex:auth-secret:v1";

#[derive(Clone)]
pub struct SecretCipher {
    cipher: Aes256Gcm,
    aad: &'static [u8],
}

impl SecretCipher {
    pub fn new(key: &[u8; 32]) -> Self {
        Self::with_aad(key, DEVICE_AAD)
    }

    pub fn with_aad(key: &[u8; 32], aad: &'static [u8]) -> Self {
        Self {
            cipher: Aes256Gcm::new_from_slice(key).expect("clé AES-256 de 32 octets"),
            aad,
        }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, ApiError> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::try_from(nonce_bytes.as_slice())
            .map_err(|_| ApiError::Internal("nonce invalide".into()))?;
        let ciphertext = self
            .cipher
            .encrypt(
                &nonce,
                aes_gcm::aead::Payload {
                    msg: plaintext.as_bytes(),
                    aad: self.aad,
                },
            )
            .map_err(|e| ApiError::Internal(format!("chiffrement secret: {e}")))?;
        let mut out = nonce_bytes.to_vec();
        out.extend_from_slice(&ciphertext);
        Ok(STANDARD.encode(out))
    }

    pub fn decrypt(&self, encoded: &str) -> Result<String, ApiError> {
        let bytes = STANDARD
            .decode(encoded.trim())
            .map_err(|_| ApiError::Internal("secret illisible (base64)".into()))?;
        if bytes.len() < 12 + 16 {
            return Err(ApiError::Internal("secret trop court".into()));
        }
        let (nonce_bytes, ciphertext) = bytes.split_at(12);
        let nonce = Nonce::try_from(nonce_bytes)
            .map_err(|_| ApiError::Internal("nonce invalide".into()))?;
        let plaintext = self
            .cipher
            .decrypt(
                &nonce,
                aes_gcm::aead::Payload {
                    msg: ciphertext,
                    aad: self.aad,
                },
            )
            .map_err(|_| ApiError::Internal("échec du déchiffrement du secret".into()))?;
        String::from_utf8(plaintext).map_err(|_| ApiError::Internal("secret non UTF-8".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_et_mauvaise_cle() {
        let key = [7u8; 32];
        let cipher = SecretCipher::new(&key);
        let secret = "motdepasse-TrèsSecret!";
        let enc = cipher.encrypt(secret).unwrap();
        assert_ne!(enc, secret);
        assert_eq!(cipher.decrypt(&enc).unwrap(), secret);
        // Mauvaise clé : le tag GCM échoue.
        let other = SecretCipher::new(&[9u8; 32]);
        assert!(other.decrypt(&enc).is_err());
        // Chiffré altéré : échec.
        let mut bytes = STANDARD.decode(&enc).unwrap();
        bytes[13] ^= 0xff;
        assert!(cipher.decrypt(&STANDARD.encode(bytes)).is_err());
    }
}
