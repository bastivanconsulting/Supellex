//! DTOs d'échange API. Toute entrée est validée (validator) avant traitement.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::auth::Role;

// ---------- Auth ----------

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    /// E-mail (source local) ou identifiant annuaire (source LDAP, ex. uid).
    #[validate(length(min = 3, max = 254))]
    pub email: String,
    // min 1 : la longueur minimale ne s'applique qu'à la création d'un mot de
    // passe local ; ici c'est le hash (local) ou l'annuaire (LDAP) qui décide.
    #[validate(length(min = 1, max = 256))]
    pub password: String,
    /// Source d'auth : None ou "local" = compte local ; sinon slug d'un
    /// fournisseur LDAP actif (OIDC passe par /auth/oidc/*, pas par ici).
    pub source: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub must_change_password: bool,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ChangePasswordRequest {
    #[validate(length(min = 8, max = 256))]
    pub old_password: String,
    #[validate(length(min = 12, max = 256, message = "12 caractères minimum"))]
    pub new_password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateUserRequest {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 1, max = 200))]
    pub display_name: String,
    #[validate(length(min = 12, max = 256))]
    pub password: String,
    pub role: Role,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateUserRequest {
    #[validate(length(min = 1, max = 200))]
    pub display_name: Option<String>,
    pub role: Option<Role>,
    pub disabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct UserDto {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: Role,
    pub disabled: bool,
    pub must_change_password: bool,
    /// 'local' ou slug du fournisseur d'auth qui a provisionné le compte.
    pub auth_source: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

// ---------- Sources d'auth externes (LDAP / OIDC) ----------

use crate::auth_providers::ProviderKind;

/// Vue publique d'une source d'auth (page de login, aucune donnée sensible).
#[derive(Debug, Serialize)]
pub struct PublicAuthProvider {
    pub slug: String,
    pub label: String,
    /// "local" | "ldap" | "oidc"
    pub kind: String,
    /// Label du champ identifiant pour un fournisseur LDAP (ex. "uid").
    pub login_label: Option<String>,
}

/// Vue admin d'un fournisseur (config avec secrets masqués).
#[derive(Debug, Serialize)]
pub struct AuthProviderDto {
    pub id: Uuid,
    pub slug: String,
    pub label: String,
    pub kind: String,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub last_test_at: Option<DateTime<Utc>>,
    pub last_test_ok: Option<bool>,
    pub created_at: DateTime<Utc>,
}

fn valid_slug(s: &str) -> bool {
    s.len() >= 2
        && s.len() <= 63
        && s.chars()
            .next()
            .map(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            .unwrap_or(false)
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertAuthProviderRequest {
    #[validate(custom(function = "validate_slug"))]
    pub slug: String,
    #[validate(length(min = 1, max = 100))]
    pub label: String,
    pub kind: ProviderKind,
    pub enabled: Option<bool>,
    /// Config JSONB du fournisseur ; champs secrets en clair (seront chiffrés
    /// au repos) ou "enc:..." (inchangés), ou absents (inchangés en update).
    pub config: serde_json::Value,
}

fn validate_slug(s: &str) -> Result<(), validator::ValidationError> {
    if valid_slug(s) {
        Ok(())
    } else {
        Err(validator::ValidationError::new(
            "slug invalide (2-63 caractères, minuscules/chiffres/tirets, commence par une lettre)",
        ))
    }
}

#[derive(Debug, Serialize)]
pub struct ProviderTestResponse {
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct OidcStartResponse {
    pub authorization_url: String,
}

// ---------- People ----------

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertPersonRequest {
    #[validate(length(min = 1, max = 100))]
    pub first_name: String,
    #[validate(length(min = 1, max = 100))]
    pub last_name: String,
    #[validate(email)]
    pub email: Option<String>,
    #[validate(length(max = 40))]
    pub phone: Option<String>,
    #[validate(length(max = 100))]
    pub department: Option<String>,
    #[validate(length(max = 100))]
    pub job_title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PersonDto {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub job_title: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ---------- Locations ----------

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertLocationRequest {
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    #[validate(custom(function = "validate_location_kind"))]
    pub kind: String,
    pub parent_id: Option<Uuid>,
    #[validate(length(max = 500))]
    pub address: Option<String>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
}

fn validate_location_kind(kind: &str) -> Result<(), validator::ValidationError> {
    match kind {
        "site" | "building" | "room" | "rack" => Ok(()),
        _ => Err(validator::ValidationError::new("kind invalide")),
    }
}

#[derive(Debug, Serialize)]
pub struct LocationDto {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<Uuid>,
    pub address: Option<String>,
    pub description: Option<String>,
}

// ---------- Assets ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    Laptop,
    Desktop,
    Server,
    Switch,
    Router,
    Firewall,
    AccessPoint,
    Printer,
    Phone,
    Monitor,
    Nas,
    Ups,
    Appliance,
    License,
    Other,
}

impl AssetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Laptop => "laptop",
            Self::Desktop => "desktop",
            Self::Server => "server",
            Self::Switch => "switch",
            Self::Router => "router",
            Self::Firewall => "firewall",
            Self::AccessPoint => "access_point",
            Self::Printer => "printer",
            Self::Phone => "phone",
            Self::Monitor => "monitor",
            Self::Nas => "nas",
            Self::Ups => "ups",
            Self::Appliance => "appliance",
            Self::License => "license",
            Self::Other => "other",
        }
    }
    pub fn from_db(s: &str) -> Option<Self> {
        Some(match s {
            "laptop" => Self::Laptop,
            "desktop" => Self::Desktop,
            "server" => Self::Server,
            "switch" => Self::Switch,
            "router" => Self::Router,
            "firewall" => Self::Firewall,
            "access_point" => Self::AccessPoint,
            "printer" => Self::Printer,
            "phone" => Self::Phone,
            "monitor" => Self::Monitor,
            "nas" => Self::Nas,
            "ups" => Self::Ups,
            "appliance" => Self::Appliance,
            "license" => Self::License,
            "other" => Self::Other,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetStatus {
    InStock,
    Deployed,
    Repair,
    Retired,
    Lost,
}

impl AssetStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InStock => "in_stock",
            Self::Deployed => "deployed",
            Self::Repair => "repair",
            Self::Retired => "retired",
            Self::Lost => "lost",
        }
    }
    pub fn from_db(s: &str) -> Option<Self> {
        Some(match s {
            "in_stock" => Self::InStock,
            "deployed" => Self::Deployed,
            "repair" => Self::Repair,
            "retired" => Self::Retired,
            "lost" => Self::Lost,
            _ => return None,
        })
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertAssetRequest {
    #[validate(length(min = 1, max = 50))]
    pub asset_tag: String,
    pub r#type: AssetType,
    pub status: Option<AssetStatus>,
    #[validate(length(max = 100))]
    pub manufacturer: Option<String>,
    #[validate(length(max = 100))]
    pub model: Option<String>,
    #[validate(length(max = 100))]
    pub serial_number: Option<String>,
    #[validate(length(max = 253))]
    pub hostname: Option<String>,
    #[validate(length(max = 100))]
    pub os: Option<String>,
    pub purchase_date: Option<NaiveDate>,
    pub warranty_end: Option<NaiveDate>,
    pub purchase_price: Option<f64>,
    pub location_id: Option<Uuid>,
    pub assigned_to: Option<Uuid>,
    #[validate(length(max = 4000))]
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AssetDto {
    pub id: Uuid,
    pub asset_tag: String,
    pub r#type: AssetType,
    pub status: AssetStatus,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub purchase_date: Option<NaiveDate>,
    pub warranty_end: Option<NaiveDate>,
    pub purchase_price: Option<f64>,
    pub location_id: Option<Uuid>,
    pub assigned_to: Option<Uuid>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct AssignAssetRequest {
    pub person_id: Option<Uuid>, // None = retour en stock
    #[validate(length(max = 500))]
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AssetAssignmentDto {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub person_id: Option<Uuid>,
    pub assigned_at: DateTime<Utc>,
    pub returned_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
}

// ---------- IPAM ----------

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertSpaceRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IpSpaceDto {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertVlanDomainRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VlanDomainDto {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertVlanRequest {
    #[validate(range(min = 1, max = 4094))]
    pub vid: i32,
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    /// Domaine VLAN (VID unique par domaine). Défaut : domaine « Default ».
    pub vlan_domain_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct VlanDto {
    pub id: Uuid,
    pub vlan_domain_id: Uuid,
    pub vid: i32,
    pub name: String,
    pub description: Option<String>,
    pub subnets_count: i64,
    pub used_count: i64,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertSubnetRequest {
    pub space_id: Uuid,
    #[validate(length(min = 1, max = 64))]
    pub cidr: String,
    #[validate(length(max = 100))]
    pub name: Option<String>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    pub gateway: Option<String>,
    pub vlan_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub dns_servers: Option<Vec<String>>,
    pub is_pool: Option<bool>,
    /// Sous-réseau parent (imbrication façon PHPIPAM).
    pub master_subnet_id: Option<Uuid>,
    /// Participe au balayage ping automatique.
    pub ping_scan: Option<bool>,
    /// Champs personnalisés (définitions dans field_defs, entité subnet).
    pub custom_fields: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SubnetDto {
    pub id: Uuid,
    pub space_id: Uuid,
    pub cidr: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub gateway: Option<String>,
    pub vlan_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub dns_servers: Vec<String>,
    pub is_pool: bool,
    pub master_subnet_id: Option<Uuid>,
    pub ping_scan: bool,
    pub custom_fields: serde_json::Value,
    pub total_addresses: String, // u128 en string pour le JSON
    pub usable_addresses: String,
    pub used_count: i64,
    pub free_count: i64,
    pub allocated_count: i64,
    pub reserved_count: i64,
    pub dhcp_count: i64,
    pub offline_count: i64,
    pub children_count: i64,
    pub utilization_pct: f64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpStatus {
    Free,
    Allocated,
    Reserved,
    Dhcp,
    Gateway,
    Offline,
}

impl IpStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Allocated => "allocated",
            Self::Reserved => "reserved",
            Self::Dhcp => "dhcp",
            Self::Gateway => "gateway",
            Self::Offline => "offline",
        }
    }
    pub fn from_db(s: &str) -> Option<Self> {
        Some(match s {
            "free" => Self::Free,
            "allocated" => Self::Allocated,
            "reserved" => Self::Reserved,
            "dhcp" => Self::Dhcp,
            "gateway" => Self::Gateway,
            "offline" => Self::Offline,
            _ => return None,
        })
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct AllocateIpRequest {
    pub status: Option<IpStatus>, // défaut: allocated
    #[validate(length(max = 253))]
    pub hostname: Option<String>,
    pub mac: Option<String>,
    pub asset_id: Option<Uuid>,
    pub person_id: Option<Uuid>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    /// Champs personnalisés (validés contre field_defs côté handler).
    pub custom_fields: Option<serde_json::Value>,
    /// Tags à associer.
    pub tag_ids: Option<Vec<Uuid>>,
    /// Si absent : prochaine IP libre calculée automatiquement.
    pub address: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateIpRequest {
    pub status: Option<IpStatus>,
    #[validate(length(max = 253))]
    pub hostname: Option<String>,
    pub mac: Option<String>,
    pub asset_id: Option<Uuid>,
    pub person_id: Option<Uuid>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    pub custom_fields: Option<serde_json::Value>,
    pub tag_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Serialize)]
pub struct IpAddressDto {
    pub id: Uuid,
    pub subnet_id: Uuid,
    pub address: String,
    pub status: IpStatus,
    pub hostname: Option<String>,
    pub mac: Option<String>,
    pub asset_id: Option<Uuid>,
    pub person_id: Option<Uuid>,
    pub description: Option<String>,
    pub custom_fields: serde_json::Value,
    pub tags: Vec<TagDto>,
    pub last_ping_ok: Option<bool>,
    pub last_ping_at: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

// ---------- Tags ----------

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertTagRequest {
    #[validate(length(min = 1, max = 60))]
    pub name: String,
    /// Couleur hexadécimale #RRGGBB (validée dans le handler).
    pub color: Option<String>,
}

pub fn is_valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit())
}

#[derive(Debug, Serialize)]
pub struct TagDto {
    pub id: Uuid,
    pub name: String,
    pub color: String,
}

// ---------- Plages DHCP ----------

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertDhcpRangeRequest {
    pub subnet_id: Uuid,
    #[validate(length(min = 1, max = 64))]
    pub start_addr: String,
    #[validate(length(min = 1, max = 64))]
    pub end_addr: String,
    #[validate(length(max = 100))]
    pub name: Option<String>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DhcpRangeDto {
    pub id: Uuid,
    pub subnet_id: Uuid,
    pub start_addr: String,
    pub end_addr: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

// ---------- NAT ----------

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertNatRuleRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    pub src_subnet_id: Option<Uuid>,
    pub src_address: Option<String>,
    pub dst_address: Option<String>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NatRuleDto {
    pub id: Uuid,
    pub name: String,
    pub src_subnet_id: Option<Uuid>,
    pub src_address: Option<String>,
    pub dst_address: Option<String>,
    pub description: Option<String>,
}

// ---------- Champs personnalisés ----------

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertFieldDefRequest {
    #[validate(custom(function = "validate_field_entity"))]
    pub entity: String,
    #[validate(length(min = 1, max = 60))]
    pub name: String,
    #[validate(custom(function = "validate_field_type"))]
    pub field_type: String,
    /// Valeurs possibles si field_type = select.
    pub options: Option<Vec<String>>,
    pub required: Option<bool>,
}

fn validate_field_entity(e: &str) -> Result<(), validator::ValidationError> {
    match e {
        "ip_address" | "subnet" | "asset" => Ok(()),
        _ => Err(validator::ValidationError::new("entité invalide")),
    }
}

fn validate_field_type(t: &str) -> Result<(), validator::ValidationError> {
    match t {
        "text" | "number" | "date" | "select" => Ok(()),
        _ => Err(validator::ValidationError::new("type invalide")),
    }
}

#[derive(Debug, Serialize)]
pub struct FieldDefDto {
    pub id: Uuid,
    pub entity: String,
    pub name: String,
    pub field_type: String,
    pub options: Vec<String>,
    pub required: bool,
}

// ---------- Résultat de scan / recherche ----------

#[derive(Debug, Serialize)]
pub struct ScanResultDto {
    pub scanned: usize,
    pub online: usize,
    pub offline: usize,
    pub new_ips: usize,
    pub mac_filled: usize,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub kind: &'static str, // ip | subnet | asset | person | vlan
    pub id: String,
    pub label: String,
    pub detail: String,
}

// ---------- Audit ----------

#[derive(Debug, Serialize)]
pub struct AuditEntryDto {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub actor_email: Option<String>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub details: serde_json::Value,
    pub ip: Option<String>,
}

// ---------- Pagination / filtres ----------

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl ListQuery {
    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
    pub fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }
}

#[derive(Debug, Serialize)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

// ---------- Devices (appareils réseau : VyOS, Cisco, MikroTik) ----------

fn validate_platform(p: &str) -> Result<(), validator::ValidationError> {
    if crate::provision::Platform::from_db(p).is_some() {
        Ok(())
    } else {
        Err(validator::ValidationError::new(
            "plateforme inconnue (vyos|cisco|mikrotik)",
        ))
    }
}

fn validate_transport(t: &str) -> Result<(), validator::ValidationError> {
    if t == "ssh" || t == "api" {
        Ok(())
    } else {
        Err(validator::ValidationError::new(
            "transport inconnu (ssh|api)",
        ))
    }
}

/// DTO exposé : jamais de secret (ni mot de passe ni clé privée).
#[derive(Debug, Serialize)]
pub struct DeviceDto {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub platform: String,
    pub transport: String,
    pub username: String,
    pub secret_kind: String,
    pub uplink_interface: Option<String>,
    pub last_test_at: Option<DateTime<Utc>>,
    pub last_test_ok: Option<bool>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_sync_ok: Option<bool>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertDeviceRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(min = 1, max = 255))]
    pub host: String,
    pub port: Option<u16>,
    #[validate(custom(function = "validate_platform"))]
    pub platform: String,
    #[validate(custom(function = "validate_transport"))]
    pub transport: String,
    #[validate(length(min = 1, max = 100))]
    pub username: String,
    #[validate(length(min = 1, max = 4096))]
    pub password: Option<String>,
    #[validate(length(min = 1, max = 8192))]
    pub ssh_key: Option<String>,
    #[validate(length(max = 64))]
    pub uplink_interface: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ProvisionRequest {
    #[validate(length(max = 200))]
    pub vlan_ids: Vec<Uuid>,
    #[validate(length(max = 200))]
    pub subnet_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct TestDeviceResponse {
    pub ok: bool,
    pub output: String,
}

#[derive(Debug, Serialize)]
pub struct ProvisionStepResult {
    pub command: String,
    pub ok: bool,
    pub output: String,
}

#[derive(Debug, Serialize)]
pub struct ProvisionResponse {
    pub ok: bool,
    pub steps: Vec<ProvisionStepResult>,
}
