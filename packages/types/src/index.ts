// Types partagés front <-> API (miroir des DTOs Rust serde).

export type Role = "viewer" | "operator" | "admin";

export interface User {
  id: string;
  email: string;
  display_name: string;
  role: Role;
  disabled: boolean;
  must_change_password: boolean;
  created_at: string;
  last_login_at: string | null;
}

export interface Person {
  id: string;
  first_name: string;
  last_name: string;
  email: string | null;
  phone: string | null;
  department: string | null;
  job_title: string | null;
  created_at: string;
}

export type LocationKind = "site" | "building" | "room" | "rack";

export interface Location {
  id: string;
  name: string;
  kind: LocationKind;
  parent_id: string | null;
  address: string | null;
  description: string | null;
}

export type AssetType =
  | "laptop" | "desktop" | "server" | "switch" | "router" | "firewall"
  | "access_point" | "printer" | "phone" | "monitor" | "nas" | "ups"
  | "appliance" | "license" | "other";

export type AssetStatus = "in_stock" | "deployed" | "repair" | "retired" | "lost";

export interface Asset {
  id: string;
  asset_tag: string;
  type: AssetType;
  status: AssetStatus;
  manufacturer: string | null;
  model: string | null;
  serial_number: string | null;
  hostname: string | null;
  os: string | null;
  purchase_date: string | null;
  warranty_end: string | null;
  purchase_price: number | null;
  location_id: string | null;
  assigned_to: string | null;
  notes: string | null;
  created_at: string;
  updated_at: string;
}

export interface AssetAssignment {
  id: string;
  asset_id: string;
  person_id: string | null;
  assigned_at: string;
  returned_at: string | null;
  note: string | null;
}

export interface IpSpace {
  id: string;
  name: string;
  description: string | null;
}

export interface VlanDomain {
  id: string;
  name: string;
  description: string | null;
}

export interface Vlan {
  id: string;
  vlan_domain_id: string;
  vid: number;
  name: string;
  description: string | null;
  subnets_count: number;
  used_count: number;
}

export interface Subnet {
  id: string;
  space_id: string;
  cidr: string;
  name: string | null;
  description: string | null;
  gateway: string | null;
  vlan_id: string | null;
  location_id: string | null;
  dns_servers: string[];
  is_pool: boolean;
  master_subnet_id: string | null;
  ping_scan: boolean;
  custom_fields: Record<string, unknown>;
  total_addresses: string;
  usable_addresses: string;
  used_count: number;
  free_count: number;
  allocated_count: number;
  reserved_count: number;
  dhcp_count: number;
  offline_count: number;
  children_count: number;
  utilization_pct: number;
  created_at: string;
}

export type IpStatus =
  | "free"
  | "allocated"
  | "reserved"
  | "dhcp"
  | "gateway"
  | "offline";

export interface Tag {
  id: string;
  name: string;
  color: string;
}

export interface IpAddress {
  id: string;
  subnet_id: string;
  address: string;
  status: IpStatus;
  hostname: string | null;
  mac: string | null;
  asset_id: string | null;
  person_id: string | null;
  description: string | null;
  custom_fields: Record<string, unknown>;
  tags: Tag[];
  last_ping_ok: boolean | null;
  last_ping_at: string | null;
  last_seen: string | null;
  updated_at: string;
}

export interface DhcpRange {
  id: string;
  subnet_id: string;
  start_addr: string;
  end_addr: string;
  name: string | null;
  description: string | null;
}

export interface NatRule {
  id: string;
  name: string;
  src_subnet_id: string | null;
  src_address: string | null;
  dst_address: string | null;
  description: string | null;
}

export interface FieldDef {
  id: string;
  entity: string;
  name: string;
  field_type: "text" | "number" | "date" | "select";
  options: string[];
  required: boolean;
}

export interface ScanResult {
  scanned: number;
  online: number;
  offline: number;
  new_ips: number;
  mac_filled: number;
}

export interface SearchHit {
  kind: "ip" | "subnet" | "asset" | "person" | "vlan" | string;
  id: string;
  label: string;
  detail: string;
}

export interface AuditEntry {
  id: number;
  at: string;
  actor_email: string | null;
  action: string;
  entity_type: string;
  entity_id: string | null;
  details: Record<string, unknown>;
  ip: string | null;
}

export interface Paged<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
}

export interface ApiErrorBody {
  error: string;
}

// ---------- Appareils réseau ----------

export type DevicePlatform = "vyos" | "cisco" | "mikrotik";
export type DeviceTransport = "ssh" | "api";

export interface Device {
  id: string;
  name: string;
  host: string;
  port: number;
  platform: DevicePlatform;
  transport: DeviceTransport;
  username: string;
  secret_kind: "password" | "ssh_key";
  uplink_interface: string | null;
  last_test_at: string | null;
  last_test_ok: boolean | null;
  last_sync_at: string | null;
  last_sync_ok: boolean | null;
  created_at: string;
}

export interface UpsertDeviceRequest {
  name: string;
  host: string;
  port?: number;
  platform: DevicePlatform;
  transport: DeviceTransport;
  username: string;
  password?: string;
  ssh_key?: string;
  uplink_interface?: string | null;
}

export interface TestDeviceResponse {
  ok: boolean;
  output: string;
}

export interface ProvisionStepResult {
  command: string;
  ok: boolean;
  output: string;
}

export interface ProvisionResponse {
  ok: boolean;
  steps: ProvisionStepResult[];
}

// ---------- Sources d'authentification (LDAP / OIDC) ----------

export type AuthProviderKind = "ldap" | "oidc";

/** Vue publique (page de login) : aucune donnée sensible. */
export interface PublicAuthProvider {
  slug: string;
  label: string;
  kind: "local" | AuthProviderKind;
  /** Label du champ identifiant pour un fournisseur LDAP (ex. "uid"). */
  login_label: string | null;
}

/** Vue admin : config avec secrets masqués ("••••••"). */
export interface AuthProvider {
  id: string;
  slug: string;
  label: string;
  kind: AuthProviderKind;
  enabled: boolean;
  config: Record<string, unknown>;
  last_test_at: string | null;
  last_test_ok: boolean | null;
  created_at: string;
}

export interface UpsertAuthProviderRequest {
  slug: string;
  label: string;
  kind: AuthProviderKind;
  enabled?: boolean;
  config: Record<string, unknown>;
}

export interface ProviderTestResponse {
  ok: boolean;
  detail: string;
}

export interface OidcStartResponse {
  authorization_url: string;
}
