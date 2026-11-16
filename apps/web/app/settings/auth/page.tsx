"use client";

import { useCallback, useEffect, useState } from "react";

import { api } from "@/lib/api-client";
import { Shell } from "@/components/sidebar";
import {
  Badge,
  Button,
  ConfirmDialog,
  ErrorBox,
  Field,
  Icon,
  Input,
  Modal,
  PageTitle,
  Select,
  Spinner,
  Table,
} from "@/components/ui";
import type {
  AuthProvider,
  AuthProviderKind,
  ProviderTestResponse,
  Role,
  UpsertAuthProviderRequest,
} from "@supellex/types";

interface FormState {
  slug: string;
  label: string;
  kind: AuthProviderKind;
  enabled: boolean;
  // LDAP
  url: string;
  base_dn: string;
  bind_dn: string;
  bind_password: string;
  user_filter: string;
  user_dn_template: string;
  login_attribute: string;
  attr_email: string;
  attr_display_name: string;
  attr_groups: string;
  starttls: boolean;
  // OIDC
  issuer_url: string;
  client_id: string;
  client_secret: string;
  redirect_uri: string;
  scopes: string;
  email_claim: string;
  name_claim: string;
  roles_claim: string;
  // commun
  tls_allow_insecure: boolean;
  role_map_text: string;
  default_role: Role;
  jit_provision: boolean;
  sync_roles: boolean;
}

const EMPTY: FormState = {
  slug: "",
  label: "",
  kind: "ldap",
  enabled: true,
  url: "",
  base_dn: "",
  bind_dn: "",
  bind_password: "",
  user_filter: "(&(objectClass=person)(uid={login}))",
  user_dn_template: "",
  login_attribute: "uid",
  attr_email: "mail",
  attr_display_name: "displayName",
  attr_groups: "memberOf",
  starttls: false,
  issuer_url: "",
  client_id: "",
  client_secret: "",
  redirect_uri: "",
  scopes: "openid email profile",
  email_claim: "email",
  name_claim: "name",
  roles_claim: "roles",
  tls_allow_insecure: false,
  role_map_text: "",
  default_role: "viewer",
  jit_provision: true,
  sync_roles: true,
};

const ROLES: Role[] = ["viewer", "operator", "admin"];
const ROLE_LABEL: Record<Role, string> = {
  viewer: "viewer (lecture)",
  operator: "operator (lecture + écriture)",
  admin: "admin (tout)",
};

function parseRoleMap(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const l = line.trim();
    if (!l || l.startsWith("#")) continue;
    const eq = l.indexOf("=");
    if (eq <= 0) continue;
    out[l.slice(0, eq).trim()] = l.slice(eq + 1).trim();
  }
  return out;
}

function buildConfig(f: FormState): Record<string, unknown> {
  const role_map = parseRoleMap(f.role_map_text);
  const base = {
    role_map,
    default_role: f.default_role,
    jit_provision: f.jit_provision,
    sync_roles: f.sync_roles,
    tls_allow_insecure: f.tls_allow_insecure,
  };
  if (f.kind === "ldap") {
    return {
      ...base,
      url: f.url,
      base_dn: f.base_dn,
      ...(f.bind_dn ? { bind_dn: f.bind_dn } : {}),
      ...(f.bind_password ? { bind_password: f.bind_password } : {}),
      user_filter: f.user_filter,
      ...(f.user_dn_template ? { user_dn_template: f.user_dn_template } : {}),
      login_attribute: f.login_attribute,
      attr_email: f.attr_email,
      attr_display_name: f.attr_display_name,
      attr_groups: f.attr_groups,
      starttls: f.starttls,
    };
  }
  return {
    ...base,
    issuer_url: f.issuer_url,
    client_id: f.client_id,
    ...(f.client_secret ? { client_secret: f.client_secret } : {}),
    ...(f.redirect_uri ? { redirect_uri: f.redirect_uri } : {}),
    scopes: f.scopes,
    email_claim: f.email_claim,
    name_claim: f.name_claim,
    roles_claim: f.roles_claim,
  };
}

function formFromProvider(p: AuthProvider): FormState {
  const c = p.config;
  const str = (k: string) => (typeof c[k] === "string" ? (c[k] as string) : "");
  const bool = (k: string) => c[k] === true;
  const roleMap = (c.role_map ?? {}) as Record<string, string>;
  return {
    ...EMPTY,
    slug: p.slug,
    label: p.label,
    kind: p.kind,
    enabled: p.enabled,
    url: str("url"),
    base_dn: str("base_dn"),
    bind_dn: str("bind_dn"),
    bind_password: "", // secret masqué côté API : vide = inchangé
    user_filter: str("user_filter") || EMPTY.user_filter,
    user_dn_template: str("user_dn_template"),
    login_attribute: str("login_attribute") || "uid",
    attr_email: str("attr_email") || "mail",
    attr_display_name: str("attr_display_name") || "displayName",
    attr_groups: str("attr_groups") || "memberOf",
    starttls: bool("starttls"),
    issuer_url: str("issuer_url"),
    client_id: str("client_id"),
    client_secret: "",
    redirect_uri: str("redirect_uri"),
    scopes: str("scopes") || "openid email profile",
    email_claim: str("email_claim") || "email",
    name_claim: str("name_claim") || "name",
    roles_claim: str("roles_claim") || "roles",
    tls_allow_insecure: bool("tls_allow_insecure"),
    role_map_text: Object.entries(roleMap)
      .map(([k, v]) => `${k} = ${v}`)
      .join("\n"),
    default_role: (ROLES.includes(c.default_role as Role)
      ? c.default_role
      : "viewer") as Role,
    jit_provision: c.jit_provision !== false,
    sync_roles: c.sync_roles !== false,
  };
}

function fmtDate(iso: string | null): string {
  return iso
    ? new Date(iso).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" })
    : "—";
}

export default function AuthSettingsPage() {
  const [providers, setProviders] = useState<AuthProvider[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<AuthProvider | null>(null);
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState<FormState>(EMPTY);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<Record<string, ProviderTestResponse>>({});
  const [deleteFor, setDeleteFor] = useState<AuthProvider | null>(null);
  const [deleting, setDeleting] = useState(false);

  const reload = useCallback(async () => {
    const p = await api.get<AuthProvider[]>("/auth-providers");
    setProviders(p);
  }, []);

  useEffect(() => {
    reload().catch((e) => setError(e instanceof Error ? e.message : "erreur"));
  }, [reload]);

  function openCreate() {
    setEditing(null);
    setForm(EMPTY);
    setCreating(true);
  }

  function openEdit(p: AuthProvider) {
    setCreating(false);
    setEditing(p);
    setForm(formFromProvider(p));
  }

  function set<K extends keyof FormState>(k: K, v: FormState[K]) {
    setForm((f) => ({ ...f, [k]: v }));
  }

  async function save() {
    setSaving(true);
    setError(null);
    const body: UpsertAuthProviderRequest = {
      slug: form.slug.trim(),
      label: form.label.trim(),
      kind: form.kind,
      enabled: form.enabled,
      config: buildConfig(form),
    };
    try {
      if (editing) {
        await api.put(`/auth-providers/${editing.id}`, body);
      } else {
        await api.post("/auth-providers", body);
      }
      setCreating(false);
      setEditing(null);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    } finally {
      setSaving(false);
    }
  }

  async function confirmRemove() {
    if (!deleteFor) return;
    setDeleting(true);
    try {
      await api.delete(`/auth-providers/${deleteFor.id}`);
      setDeleteFor(null);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    } finally {
      setDeleting(false);
    }
  }

  async function test(id: string) {
    setTesting(id);
    try {
      const r = await api.post<ProviderTestResponse>(`/auth-providers/${id}/test`, {});
      setTestResult((m) => ({ ...m, [id]: r }));
      await reload();
    } catch (e) {
      setTestResult((m) => ({
        ...m,
        [id]: { ok: false, detail: e instanceof Error ? e.message : "erreur" },
      }));
    } finally {
      setTesting(null);
    }
  }

  const modalOpen = creating || editing !== null;

  return (
    <Shell>
      <PageTitle title="Authentification">
        <Button onClick={openCreate}>Ajouter une source</Button>
      </PageTitle>
      <p className="mb-6 max-w-2xl text-muted">
        Les comptes locaux restent toujours disponibles. Ajoute un annuaire LDAP ou un fournisseur
        SSO (OIDC)&nbsp;: les utilisateurs sont créés à la première connexion et leur rôle suit le
        mapping ci-dessous.
      </p>
      {error && (
        <div className="mb-4 max-w-2xl">
          <ErrorBox message={error} />
        </div>
      )}
      {providers === null ? (
        <Spinner />
      ) : providers.length === 0 ? (
        <p className="text-muted">Aucune source externe configurée. La connexion se fait en local.</p>
      ) : (
        <Table
          columns={["Libellé", "Slug", "Type", "État", "Dernier test", ""]}
          rows={providers.map((p) => [
            <span key="l" className="font-medium">{p.label}</span>,
            <span key="s" className="font-mono text-xs">{p.slug}</span>,
            <Badge key="t" tone="blue">{p.kind === "ldap" ? "LDAP" : "SSO (OIDC)"}</Badge>,
            <Badge key="e" tone={p.enabled ? "green" : "gray"}>
              {p.enabled ? "activée" : "désactivée"}
            </Badge>,
            <span key="d" className="whitespace-nowrap text-xs text-muted">
              {p.last_test_at ? (
                <>
                  <Badge tone={p.last_test_ok ? "green" : "red"}>
                    {p.last_test_ok ? "ok" : "échec"}
                  </Badge>{" "}
                  {fmtDate(p.last_test_at)}
                </>
              ) : (
                "—"
              )}
              {testResult[p.id] && (
                <span className="mt-1 block max-w-xs whitespace-normal">
                  {testResult[p.id].detail}
                </span>
              )}
            </span>,
            <span key="a" className="flex items-center gap-1 whitespace-nowrap">
              <Button size="sm" onClick={() => test(p.id)} disabled={testing === p.id}>
                <Icon name="bolt" /> {testing === p.id ? "test…" : "Tester"}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => openEdit(p)}>
                <Icon name="edit" /> Modifier
              </Button>
              <Button size="sm" variant="danger" onClick={() => setDeleteFor(p)}>
                <Icon name="trash" /> Supprimer
              </Button>
            </span>,
          ])}
        />
      )}

      {modalOpen && (
        <Modal
          title={editing ? `Modifier ${editing.label}` : "Ajouter une source d'authentification"}
          wide
          onClose={() => {
            setCreating(false);
            setEditing(null);
          }}
        >
          <Field label="Type">
            <Select
              value={form.kind}
              onChange={(v) => set("kind", v as AuthProviderKind)}
              className="w-full"
            >
              <option value="ldap">LDAP (Active Directory, OpenLDAP…)</option>
              <option value="oidc">SSO — OIDC (Keycloak, Entra ID, Auth0…)</option>
            </Select>
          </Field>
          <Field label="Libellé affiché sur la page de connexion">
            <Input value={form.label} onChange={(v) => set("label", v)} className="w-full" />
          </Field>
          <Field label="Slug (identifiant stable, ex. ldap-ad)">
            <Input
              value={form.slug}
              onChange={(v) => set("slug", v)}
              className="w-full font-mono"
              placeholder="ldap-ad"
            />
          </Field>
          <label className="mb-3 flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={form.enabled}
              onChange={(e) => set("enabled", e.target.checked)}
            />
            <span className="text-muted">source activée</span>
          </label>

          {form.kind === "ldap" ? (
            <>
              <Field label="URL (ldaps://…:636 ou ldap://…:389)">
                <Input value={form.url} onChange={(v) => set("url", v)} className="w-full" placeholder="ldaps://ldap.example.com:636" />
              </Field>
              <Field label="Base DN">
                <Input value={form.base_dn} onChange={(v) => set("base_dn", v)} className="w-full" placeholder="dc=example,dc=com" />
              </Field>
              <Field label="Bind DN du compte de service (ou DN direct ci-dessous)">
                <Input value={form.bind_dn} onChange={(v) => set("bind_dn", v)} className="w-full" placeholder="cn=svc-supellex,ou=services,dc=example,dc=com" />
              </Field>
              <Field label="Mot de passe du compte de service (chiffré au repos)">
                <Input
                  type="password"
                  value={form.bind_password}
                  onChange={(v) => set("bind_password", v)}
                  className="w-full"
                  placeholder={editing ? "•••••• (inchangé si vide)" : ""}
                />
              </Field>
              <Field label="Filtre utilisateur ({login} est échappé)">
                <Input value={form.user_filter} onChange={(v) => set("user_filter", v)} className="w-full font-mono text-xs" />
              </Field>
              <Field label={"Alternative\u00A0: DN direct (bind sans recherche), ex. uid={login},ou=people,dc=…"}>
                <Input value={form.user_dn_template} onChange={(v) => set("user_dn_template", v)} className="w-full font-mono text-xs" />
              </Field>
              <Field label="Attribut saisi comme identifiant (label du champ de connexion)">
                <Input value={form.login_attribute} onChange={(v) => set("login_attribute", v)} className="w-full" placeholder="uid" />
              </Field>
              <div className="grid grid-cols-3 gap-2">
                <Field label="Attribut e-mail">
                  <Input value={form.attr_email} onChange={(v) => set("attr_email", v)} className="w-full" />
                </Field>
                <Field label="Attribut nom affiché">
                  <Input value={form.attr_display_name} onChange={(v) => set("attr_display_name", v)} className="w-full" />
                </Field>
                <Field label="Attribut groupes">
                  <Input value={form.attr_groups} onChange={(v) => set("attr_groups", v)} className="w-full" />
                </Field>
              </div>
              <label className="mb-3 flex items-center gap-2 text-sm">
                <input type="checkbox" checked={form.starttls} onChange={(e) => set("starttls", e.target.checked)} />
                <span className="text-muted">StartTLS sur ldap://</span>
              </label>
            </>
          ) : (
            <>
              <Field label="Issuer URL (découverte .well-known/openid-configuration)">
                <Input value={form.issuer_url} onChange={(v) => set("issuer_url", v)} className="w-full" placeholder="https://sso.example.com/realms/supellex" />
              </Field>
              <Field label="Client ID">
                <Input value={form.client_id} onChange={(v) => set("client_id", v)} className="w-full" />
              </Field>
              <Field label="Client secret (chiffré au repos)">
                <Input
                  type="password"
                  value={form.client_secret}
                  onChange={(v) => set("client_secret", v)}
                  className="w-full"
                  placeholder={editing ? "•••••• (inchangé si vide)" : ""}
                />
              </Field>
              <Field label={"Redirect URI (par défaut\u00A0: PUBLIC_BASE_URL + /api/auth/oidc/callback)"}>
                <Input value={form.redirect_uri} onChange={(v) => set("redirect_uri", v)} className="w-full" placeholder="https://supellex.example.com/api/auth/oidc/callback" />
              </Field>
              <Field label="Scopes (séparés par des espaces)">
                <Input value={form.scopes} onChange={(v) => set("scopes", v)} className="w-full" />
              </Field>
              <div className="grid grid-cols-3 gap-2">
                <Field label="Claim e-mail">
                  <Input value={form.email_claim} onChange={(v) => set("email_claim", v)} className="w-full" />
                </Field>
                <Field label="Claim nom">
                  <Input value={form.name_claim} onChange={(v) => set("name_claim", v)} className="w-full" />
                </Field>
                <Field label="Claim rôles">
                  <Input value={form.roles_claim} onChange={(v) => set("roles_claim", v)} className="w-full" />
                </Field>
              </div>
            </>
          )}

          <Field label={"Mapping des rôles (une ligne par règle\u00A0: valeur = rôle)"}>
            <textarea
              value={form.role_map_text}
              onChange={(e) => set("role_map_text", e.target.value)}
              rows={3}
              className="w-full rounded-md border border-line-strong bg-surface px-3 py-2 font-mono text-xs text-ink outline-none focus:border-accent focus-visible:ring-2 focus-visible:ring-accent/30"
              placeholder={"cn=supellex-admins,ou=groups,dc=example,dc=com = admin\nsupellex-ops = operator"}
            />
          </Field>
          <Field label="Rôle par défaut (premier compte créé sans correspondance)">
            <Select value={form.default_role} onChange={(v) => set("default_role", v as Role)} className="w-full">
              {ROLES.map((r) => (
                <option key={r} value={r}>{ROLE_LABEL[r]}</option>
              ))}
            </Select>
          </Field>
          <label className="mb-3 flex items-center gap-2 text-sm">
            <input type="checkbox" checked={form.jit_provision} onChange={(e) => set("jit_provision", e.target.checked)} />
            <span className="text-muted">créer le compte automatiquement à la première connexion</span>
          </label>
          <label className="mb-3 flex items-center gap-2 text-sm">
            <input type="checkbox" checked={form.sync_roles} onChange={(e) => set("sync_roles", e.target.checked)} />
            <span className="text-muted">resynchroniser le rôle à chaque connexion depuis le mapping</span>
          </label>
          <label className="mb-4 flex items-center gap-2 text-sm">
            <input type="checkbox" checked={form.tls_allow_insecure} onChange={(e) => set("tls_allow_insecure", e.target.checked)} />
            <span className="text-muted">désactiver la vérification du certificat TLS (test uniquement)</span>
          </label>

          <div className="flex justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                setCreating(false);
                setEditing(null);
              }}
            >
              Annuler
            </Button>
            <Button onClick={save} disabled={saving || !form.slug || !form.label}>
              {saving ? "enregistrement…" : "Enregistrer"}
            </Button>
          </div>
        </Modal>
      )}

      <ConfirmDialog
        open={deleteFor !== null}
        title="Supprimer cette source ?"
        message={
          deleteFor
            ? `La source « ${deleteFor.label} » (${deleteFor.slug}) sera supprimée définitivement. Les utilisateurs provisionnés via cette source ne pourront plus se connecter avec.`
            : ""
        }
        confirmLabel="Supprimer"
        loading={deleting}
        onCancel={() => setDeleteFor(null)}
        onConfirm={confirmRemove}
      />
    </Shell>
  );
}
