//! Appareils réseau (VyOS, Cisco, MikroTik) : CRUD, test de connexion,
//! provisioning (création des VLANs / sous-réseaux sur l'appareil).
//! Les secrets sont chiffrés au repos (AES-256-GCM) et jamais renvoyés.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;
use validator::Validate;

use crate::{
    audit,
    auth::{Claims, Role, require_role},
    device_net::{self, DeviceAuth},
    error::{ApiError, ApiResult},
    models::*,
    provision::{self, Platform},
    state::AppState,
};

type DeviceRow = (
    Uuid,
    String,
    String,
    i32,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<bool>,
    Option<DateTime<Utc>>,
    Option<bool>,
    DateTime<Utc>,
);

/// Colonnes communes (chaîne constante, jamais injectée : sqlx exige des
/// littéraux statiques en 0.9).
macro_rules! select_cols {
    () => {
        "id, name, host, port, platform::text, transport::text, username, secret_cipher, secret_kind, uplink_interface, host_key_fp, last_test_at, last_test_ok, last_sync_at, last_sync_ok, created_at"
    };
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update).delete(remove))
        .route("/{id}/test", axum::routing::post(test))
        .route("/{id}/provision", axum::routing::post(provision_))
}

fn to_dto(r: DeviceRow) -> DeviceDto {
    DeviceDto {
        id: r.0,
        name: r.1,
        host: r.2,
        port: r.3 as u16,
        platform: r.4,
        transport: r.5,
        username: r.6,
        secret_kind: r.8,
        uplink_interface: r.9,
        last_test_at: r.11,
        last_test_ok: r.12,
        last_sync_at: r.13,
        last_sync_ok: r.14,
        created_at: r.15,
    }
}

async fn load_row(state: &AppState, id: Uuid) -> ApiResult<DeviceRow> {
    sqlx::query_as::<_, DeviceRow>(concat!(
        "SELECT ",
        select_cols!(),
        " FROM devices WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)
}

async fn list(State(state): State<AppState>, _claims: Claims) -> ApiResult<Json<Vec<DeviceDto>>> {
    let rows = sqlx::query_as::<_, DeviceRow>(concat!(
        "SELECT ",
        select_cols!(),
        " FROM devices ORDER BY name"
    ))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(to_dto).collect()))
}

async fn get_one(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<DeviceDto>> {
    Ok(Json(to_dto(load_row(&state, id).await?)))
}

/// Le secret n'est fourni qu'à la création ou pour le remplacer ; jamais en lecture.
fn encode_secret(state: &AppState, req: &UpsertDeviceRequest) -> ApiResult<(String, String)> {
    let (kind, plain) = if let Some(p) = &req.password {
        ("password", p.clone())
    } else if let Some(k) = &req.ssh_key {
        ("ssh_key", k.clone())
    } else {
        return Err(ApiError::Validation(
            "fournir un mot de passe ou une clé SSH privée".into(),
        ));
    };
    Ok((kind.to_string(), state.device_cipher.encrypt(&plain)?))
}

async fn create(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpsertDeviceRequest>,
) -> ApiResult<Json<DeviceDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let (kind, cipher) = encode_secret(&state, &req)?;
    let port = req
        .port
        .unwrap_or(if req.transport == "api" { 8728 } else { 22 });
    let row = sqlx::query_as::<_, DeviceRow>(concat!(
        "INSERT INTO devices (name, host, port, platform, transport, username, secret_cipher, secret_kind, uplink_interface)
         VALUES ($1, $2, $3, $4::device_platform, $5::device_transport, $6, $7, $8, $9)
         RETURNING ",
        select_cols!()
    ))
    .bind(&req.name)
    .bind(&req.host)
    .bind(port as i32)
    .bind(&req.platform)
    .bind(&req.transport)
    .bind(&req.username)
    .bind(&cipher)
    .bind(&kind)
    .bind(&req.uplink_interface)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::Conflict("appareil existe déjà".into()))?;
    audit::AuditEvent::new(Some(&claims), "device.create", "device", Some(row.0.to_string()))
        .details(json!({"name": req.name, "host": req.host, "platform": req.platform, "transport": req.transport, "secret_kind": kind}))
        .ip(None)
        .write(&state.pool)
        .await;
    Ok(Json(to_dto(row)))
}

async fn update(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertDeviceRequest>,
) -> ApiResult<Json<DeviceDto>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    // Secret : si fourni, on le remplace ; sinon on garde l'existant.
    let existing = load_row(&state, id).await?;
    let (kind, cipher) = if req.password.is_some() || req.ssh_key.is_some() {
        encode_secret(&state, &req)?
    } else {
        (existing.8.clone(), existing.7.clone())
    };
    let port = req.port.unwrap_or(existing.3 as u16);
    let row = sqlx::query_as::<_, DeviceRow>(concat!(
        "UPDATE devices SET name=$2, host=$3, port=$4, platform=$5::device_platform,
           transport=$6::device_transport, username=$7, secret_cipher=$8, secret_kind=$9,
           uplink_interface=$10, updated_at=now()
         WHERE id=$1 RETURNING ",
        select_cols!()
    ))
    .bind(id)
    .bind(&req.name)
    .bind(&req.host)
    .bind(port as i32)
    .bind(&req.platform)
    .bind(&req.transport)
    .bind(&req.username)
    .bind(&cipher)
    .bind(&kind)
    .bind(&req.uplink_interface)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::AuditEvent::new(Some(&claims), "device.update", "device", Some(id.to_string()))
        .details(json!({"name": req.name, "host": req.host, "secret_changed": req.password.is_some() || req.ssh_key.is_some()}))
        .ip(None)
        .write(&state.pool)
        .await;
    Ok(Json(to_dto(row)))
}

async fn remove(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&claims, Role::Admin)?;
    let result = sqlx::query("DELETE FROM devices WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::AuditEvent::new(
        Some(&claims),
        "device.delete",
        "device",
        Some(id.to_string()),
    )
    .details(json!({}))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn platform_of(row: &DeviceRow) -> ApiResult<Platform> {
    Platform::from_db(&row.4)
        .ok_or_else(|| ApiError::Internal("plateforme inconnue en base".into()))
}

fn auth_of<'a>(row: &'a DeviceRow, secret: &'a str) -> DeviceAuth<'a> {
    match row.8.as_str() {
        "ssh_key" => DeviceAuth::Key(secret),
        _ => DeviceAuth::Password(secret),
    }
}

/// Test de connexion : exécute une commande de lecture seule sur l'appareil.
async fn test(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TestDeviceResponse>> {
    require_role(&claims, Role::Operator)?;
    let row = load_row(&state, id).await?;
    let platform = platform_of(&row)?;
    let secret = state.device_cipher.decrypt(&row.7)?;
    let port = row.3 as u16;

    let result = async {
        match row.5.as_str() {
            "ssh" => {
                let cmd = match platform {
                    Platform::Mikrotik => "/system resource print",
                    _ => "show version",
                };
                let r = device_net::ssh_exec(
                    &row.2,
                    port,
                    &row.6,
                    auth_of(&row, &secret),
                    row.10.as_deref(),
                    cmd,
                )
                .await?;
                // TOFU : on épingle l'empreinte au premier contact réussi.
                if row.10.is_none() && !r.host_key_fp.is_empty() {
                    sqlx::query("UPDATE devices SET host_key_fp = $2 WHERE id = $1")
                        .bind(id)
                        .bind(&r.host_key_fp)
                        .execute(&state.pool)
                        .await?;
                }
                Ok::<_, ApiError>(TestDeviceResponse {
                    ok: r.exit_status == 0 && !provision::output_has_error(&r.output),
                    output: truncate(&r.output, 2000),
                })
            }
            "api" => {
                if platform != Platform::Mikrotik {
                    return Err(ApiError::Validation(
                        "le transport API n'est supporté que pour MikroTik (port 8728)".into(),
                    ));
                }
                let replies = device_net::routeros_api(
                    &row.2,
                    port,
                    &row.6,
                    &secret,
                    &[vec!["/system/resource/print".to_string()]],
                )
                .await?;
                let ok = replies.iter().all(|r| r.ok());
                let output = replies
                    .iter()
                    .flat_map(|r| r.data.clone())
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(TestDeviceResponse {
                    ok,
                    output: truncate(&output, 2000),
                })
            }
            other => Err(ApiError::Validation(format!("transport inconnu: {other}"))),
        }
    }
    .await;

    let (ok, output) = match result {
        Ok(r) => (r.ok, r.output),
        Err(e) => {
            let msg = e.to_string();
            sqlx::query(
                "UPDATE devices SET last_test_at = now(), last_test_ok = FALSE WHERE id = $1",
            )
            .bind(id)
            .execute(&state.pool)
            .await?;
            audit::AuditEvent::new(Some(&claims), "device.test", "device", Some(id.to_string()))
                .details(json!({"ok": false, "error": msg}))
                .ip(None)
                .write(&state.pool)
                .await;
            return Err(e);
        }
    };
    sqlx::query("UPDATE devices SET last_test_at = now(), last_test_ok = $2 WHERE id = $1")
        .bind(id)
        .bind(ok)
        .execute(&state.pool)
        .await?;
    audit::AuditEvent::new(Some(&claims), "device.test", "device", Some(id.to_string()))
        .details(json!({"ok": ok, "output": output}))
        .ip(None)
        .write(&state.pool)
        .await;
    Ok(Json(TestDeviceResponse { ok, output }))
}

/// Provisionnement : crée les VLANs et sous-réseaux demandés sur l'appareil.
async fn provision_(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<ProvisionRequest>,
) -> ApiResult<Json<ProvisionResponse>> {
    require_role(&claims, Role::Operator)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if req.vlan_ids.is_empty() && req.subnet_ids.is_empty() {
        return Err(ApiError::Validation(
            "sélectionner au moins un VLAN ou un sous-réseau".into(),
        ));
    }
    let row = load_row(&state, id).await?;
    let platform = platform_of(&row)?;
    let secret = state.device_cipher.decrypt(&row.7)?;
    let port = row.3 as u16;
    let uplink = row.9.clone().unwrap_or_else(|| match platform {
        Platform::Mikrotik => "ether1".into(),
        _ => "eth0".into(),
    });

    // VLANs sélectionnés.
    let vlans = sqlx::query_as::<_, (Uuid, i32, String)>(
        "SELECT id, vid, name FROM vlans WHERE id = ANY($1) ORDER BY vid",
    )
    .bind(&req.vlan_ids)
    .fetch_all(&state.pool)
    .await?;
    // Sous-réseaux sélectionnés, avec leur VLAN pour le nommage d'interface.
    let subnets = sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<i32>, Option<String>)>(
        "SELECT s.id, s.cidr::text, s.vlan_id, v.vid, v.name
         FROM subnets s LEFT JOIN vlans v ON v.id = s.vlan_id
         WHERE s.id = ANY($1) ORDER BY s.cidr",
    )
    .bind(&req.subnet_ids)
    .fetch_all(&state.pool)
    .await?;

    // Rendu des commandes : VLANs d'abord, puis les sous-réseaux.
    let mut commands: Vec<String> = Vec::new();
    for (_, vid, name) in &vlans {
        commands.extend(provision::vlan_commands(
            platform,
            *vid as u16,
            name,
            &uplink,
        ));
    }
    for (_, cidr, vlan_id, vid, vname) in &subnets {
        let (vid, vname) = match (vid, vname) {
            (Some(v), Some(n)) => (*v as u16, n.clone()),
            _ => {
                return Err(ApiError::Validation(format!(
                    "le sous-réseau {cidr} n'est rattaché à aucun VLAN : impossible de l'adresser"
                )));
            }
        };
        let _ = vlan_id;
        commands.extend(
            provision::subnet_commands(platform, cidr, vid, &vname, &uplink)
                .map_err(ApiError::Validation)?,
        );
    }
    if commands.is_empty() {
        return Err(ApiError::Validation("aucune commande générée".into()));
    }

    // Envoi selon le transport.
    let steps: Vec<ProvisionStepResult> = match row.5.as_str() {
        "ssh" => {
            let script = provision::wrap_session(platform, &commands);
            let r = device_net::ssh_exec(
                &row.2,
                port,
                &row.6,
                auth_of(&row, &secret),
                row.10.as_deref(),
                &script,
            )
            .await?;
            vec![ProvisionStepResult {
                command: script,
                ok: r.exit_status == 0 && !provision::output_has_error(&r.output),
                output: truncate(&r.output, 4000),
            }]
        }
        "api" => {
            if platform != Platform::Mikrotik {
                return Err(ApiError::Validation(
                    "le transport API n'est supporté que pour MikroTik".into(),
                ));
            }
            let api_cmds: Vec<Vec<String>> = commands
                .iter()
                .map(|c| provision::cli_to_api_words(c))
                .collect();
            let replies =
                device_net::routeros_api(&row.2, port, &row.6, &secret, &api_cmds).await?;
            commands
                .into_iter()
                .zip(replies)
                .map(|(cmd, reply)| ProvisionStepResult {
                    ok: reply.ok(),
                    output: if reply.ok() {
                        reply.data.join(" ")
                    } else {
                        reply.traps.join("; ")
                    },
                    command: cmd,
                })
                .collect()
        }
        other => return Err(ApiError::Validation(format!("transport inconnu: {other}"))),
    };

    let ok = steps.iter().all(|s| s.ok);
    sqlx::query("UPDATE devices SET last_sync_at = now(), last_sync_ok = $2 WHERE id = $1")
        .bind(id)
        .bind(ok)
        .execute(&state.pool)
        .await?;
    audit::AuditEvent::new(
        Some(&claims),
        "device.provision",
        "device",
        Some(id.to_string()),
    )
    .details(json!({
        "vlans": vlans.len(),
        "subnets": subnets.len(),
        "commands": steps.len(),
        "ok": ok,
    }))
    .ip(None)
    .write(&state.pool)
    .await;
    Ok(Json(ProvisionResponse { ok, steps }))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
