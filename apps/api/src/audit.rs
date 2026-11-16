use serde_json::Value;
use sqlx::PgPool;
use std::net::IpAddr;

use crate::auth::Claims;

/// Événement d'audit. Appelé sur chaque mutation (auth, droits, IPAM, actifs).
/// Jamais de secret ni de donnée personnelle superflue dans `details`.
///
/// Usage : `AuditEvent::new(Some(&claims), "asset.create", "asset", id)
///             .details(json!({...})).write(&pool).await`
pub struct AuditEvent<'a> {
    actor: Option<&'a Claims>,
    action: &'a str,
    entity_type: &'a str,
    entity_id: Option<String>,
    details: Value,
    ip: Option<IpAddr>,
}

impl<'a> AuditEvent<'a> {
    pub fn new(
        actor: Option<&'a Claims>,
        action: &'a str,
        entity_type: &'a str,
        entity_id: Option<String>,
    ) -> Self {
        Self {
            actor,
            action,
            entity_type,
            entity_id,
            details: Value::Null,
            ip: None,
        }
    }

    pub fn details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn ip(mut self, ip: Option<IpAddr>) -> Self {
        self.ip = ip;
        self
    }

    pub async fn write(self, pool: &PgPool) {
        let actor_id = self.actor.map(|c| c.sub);
        let actor_email = self.actor.map(|c| c.email.clone());
        if let Err(e) = sqlx::query(
            "INSERT INTO audit_log (actor_id, actor_email, action, entity_type, entity_id, details, ip)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(actor_id)
        .bind(actor_email)
        .bind(self.action)
        .bind(self.entity_type)
        .bind(self.entity_id)
        .bind(self.details)
        .bind(self.ip)
        .execute(pool)
        .await
        {
            // Un échec d'audit ne doit pas casser la requête métier, mais il est tracé.
            tracing::error!(error = %e, "échec d'écriture audit_log");
        }
    }
}
