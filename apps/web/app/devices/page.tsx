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
  Device,
  DevicePlatform,
  DeviceTransport,
  ProvisionResponse,
  Subnet,
  TestDeviceResponse,
  UpsertDeviceRequest,
  Vlan,
} from "@supellex/types";

const EMPTY: UpsertDeviceRequest = {
  name: "",
  host: "",
  port: undefined,
  platform: "vyos",
  transport: "ssh",
  username: "",
  password: "",
  uplink_interface: "",
};

const PLATFORM_LABEL: Record<DevicePlatform, string> = {
  vyos: "VyOS",
  cisco: "Cisco IOS",
  mikrotik: "MikroTik RouterOS",
};

function fmtDate(iso: string | null): string {
  return iso
    ? new Date(iso).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" })
    : "—";
}

export default function DevicesPage() {
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [vlans, setVlans] = useState<Vlan[] | null>(null);
  const [subnets, setSubnets] = useState<Subnet[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Device | null>(null);
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState<UpsertDeviceRequest>(EMPTY);
  const [secretKind, setSecretKind] = useState<"password" | "ssh_key">("password");
  const [testing, setTesting] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<TestDeviceResponse | null>(null);
  const [provisionFor, setProvisionFor] = useState<Device | null>(null);
  const [selVlans, setSelVlans] = useState<Set<string>>(new Set());
  const [selSubnets, setSelSubnets] = useState<Set<string>>(new Set());
  const [provisioning, setProvisioning] = useState(false);
  const [provisionResult, setProvisionResult] = useState<ProvisionResponse | null>(null);
  const [deleteFor, setDeleteFor] = useState<Device | null>(null);
  const [deleting, setDeleting] = useState(false);

  const reload = useCallback(async () => {
    const [d, v, s] = await Promise.all([
      api.get<Device[]>("/devices"),
      api.get<Vlan[]>("/ipam/vlans"),
      api.get<{ items: Subnet[] }>("/ipam/subnets?limit=500"),
    ]);
    setDevices(d);
    setVlans(v);
    setSubnets(s.items);
  }, []);

  useEffect(() => {
    reload().catch((e) => setError(e instanceof Error ? e.message : "erreur"));
  }, [reload]);

  async function save() {
    setError(null);
    const body: UpsertDeviceRequest = {
      ...form,
      port: form.port || undefined,
      uplink_interface: form.uplink_interface || null,
      password: secretKind === "password" ? form.password || undefined : undefined,
      ssh_key: secretKind === "ssh_key" ? (form as { ssh_key?: string }).ssh_key || undefined : undefined,
    };
    try {
      if (editing) {
        await api.put(`/devices/${editing.id}`, body);
      } else {
        await api.post("/devices", body);
      }
      setCreating(false);
      setEditing(null);
      setForm(EMPTY);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  async function confirmRemove() {
    if (!deleteFor) return;
    setDeleting(true);
    try {
      await api.delete(`/devices/${deleteFor.id}`);
      setDeleteFor(null);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    } finally {
      setDeleting(false);
    }
  }

  async function testDevice(d: Device) {
    setTesting(d.id);
    setTestResult(null);
    setError(null);
    try {
      const r = await api.post<TestDeviceResponse>(`/devices/${d.id}/test`, {});
      setTestResult(r);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    } finally {
      setTesting(null);
    }
  }

  async function runProvision() {
    if (!provisionFor) return;
    setProvisioning(true);
    setProvisionResult(null);
    setError(null);
    try {
      const r = await api.post<ProvisionResponse>(`/devices/${provisionFor.id}/provision`, {
        vlan_ids: [...selVlans],
        subnet_ids: [...selSubnets],
      });
      setProvisionResult(r);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    } finally {
      setProvisioning(false);
    }
  }

  const toggle = (set: Set<string>, id: string) => {
    const next = new Set(set);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  };

  return (
    <Shell>
      <PageTitle title="Appareils réseau">
        <Button
          onClick={() => {
            setForm(EMPTY);
            setSecretKind("password");
            setCreating(true);
          }}
        >
          <Icon name="plus" /> Nouvel appareil
        </Button>
      </PageTitle>
      {error && (
        <div className="mb-4">
          <ErrorBox message={error} />
        </div>
      )}
      {!devices ? (
        <Spinner />
      ) : devices.length === 0 ? (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-dashed border-line-strong bg-surface px-6 py-14 text-center">
          <Icon name="device" className="h-8 w-8 text-faint" />
          <p className="text-sm text-muted">
            Aucun appareil réseau enregistré. Ajoutez un routeur ou switch VyOS, Cisco ou MikroTik
            pour le piloter depuis ici.
          </p>
          <Button
            onClick={() => {
              setForm(EMPTY);
              setSecretKind("password");
              setCreating(true);
            }}
          >
            <Icon name="plus" /> Nouvel appareil
          </Button>
        </div>
      ) : (
        <Table
          columns={["Nom", "Hôte", "Plateforme", "Transport", "Identifiant", "Secret", "Dernier test", "Dernier sync", "Actions"]}
          rows={devices.map((d) => [
            <span key="n" className="flex items-center gap-2 whitespace-nowrap font-medium">
              <Icon name="device" className="h-4 w-4 text-faint" />
              {d.name}
            </span>,
            <span key="h" className="whitespace-nowrap font-mono text-sm">{d.host}:{d.port}</span>,
            <Badge key="p" tone="blue">{PLATFORM_LABEL[d.platform]}</Badge>,
            <Badge key="t" tone="gray">{d.transport === "api" ? "API" : "SSH"}</Badge>,
            <span key="u" className="whitespace-nowrap text-sm text-muted">{d.username}</span>,
            <Badge key="s" tone="gray">{d.secret_kind === "ssh_key" ? "clé SSH" : "mot de passe"}</Badge>,
            <span key="lt" className="whitespace-nowrap text-sm">
              {d.last_test_ok === null ? (
                <span className="text-faint">—</span>
              ) : (
                <Badge tone={d.last_test_ok ? "green" : "red"}>
                  {d.last_test_ok ? "ok" : "échec"} · {fmtDate(d.last_test_at)}
                </Badge>
              )}
            </span>,
            <span key="ls" className="whitespace-nowrap text-sm">
              {d.last_sync_ok === null ? (
                <span className="text-faint">—</span>
              ) : (
                <Badge tone={d.last_sync_ok ? "green" : "red"}>
                  {d.last_sync_ok ? "ok" : "échec"} · {fmtDate(d.last_sync_at)}
                </Badge>
              )}
            </span>,
            <span key="a" className="flex items-center gap-1 whitespace-nowrap">
              <Button size="sm" variant="ghost" onClick={() => testDevice(d)} disabled={testing === d.id}>
                <Icon name="bolt" /> {testing === d.id ? "test…" : "Tester"}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setProvisionFor(d);
                  setSelVlans(new Set());
                  setSelSubnets(new Set());
                  setProvisionResult(null);
                }}
              >
                <Icon name="upload" /> Pousser
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setEditing(d);
                  setSecretKind(d.secret_kind);
                  setForm({
                    name: d.name,
                    host: d.host,
                    port: d.port,
                    platform: d.platform,
                    transport: d.transport,
                    username: d.username,
                    password: "",
                    uplink_interface: d.uplink_interface ?? "",
                  });
                }}
              >
                <Icon name="edit" /> Modifier
              </Button>
              <Button size="sm" variant="danger" onClick={() => setDeleteFor(d)}>
                <Icon name="trash" /> Supprimer
              </Button>
            </span>,
          ])}
        />
      )}

      {testResult && (
        <Modal title="Résultat du test de connexion" onClose={() => setTestResult(null)} wide>
          <Badge tone={testResult.ok ? "green" : "red"}>{testResult.ok ? "connexion ok" : "échec"}</Badge>
          <pre className="mt-3 max-h-80 overflow-auto rounded-md border border-line bg-canvas p-3 font-mono text-xs">
            {testResult.output || "(sortie vide)"}
          </pre>
        </Modal>
      )}

      {(creating || editing) && (
        <Modal
          title={editing ? `Modifier ${editing.name}` : "Nouvel appareil"}
          onClose={() => {
            setCreating(false);
            setEditing(null);
          }}
          wide
        >
          <div className="grid gap-x-3 sm:grid-cols-2">
            <Field label="Nom *">
              <Input value={form.name} onChange={(v) => setForm({ ...form, name: v })} className="w-full" />
            </Field>
            <Field label="Hôte (IP ou DNS) *">
              <Input value={form.host} onChange={(v) => setForm({ ...form, host: v })} className="w-full" />
            </Field>
            <Field label="Port">
              <Input
                value={form.port?.toString() ?? ""}
                onChange={(v) => setForm({ ...form, port: v ? Number(v) : undefined })}
                className="w-full"
                placeholder={form.transport === "api" ? "8728" : "22"}
              />
            </Field>
            <Field label="Interface uplink (ex. eth0, ether1)">
              <Input
                value={form.uplink_interface ?? ""}
                onChange={(v) => setForm({ ...form, uplink_interface: v })}
                className="w-full"
              />
            </Field>
            <Field label="Plateforme">
              <Select
                value={form.platform}
                onChange={(v) => setForm({ ...form, platform: v as DevicePlatform })}
                className="w-full"
              >
                <option value="vyos">VyOS</option>
                <option value="cisco">Cisco IOS</option>
                <option value="mikrotik">MikroTik RouterOS</option>
              </Select>
            </Field>
            <Field label="Transport">
              <Select
                value={form.transport}
                onChange={(v) => setForm({ ...form, transport: v as DeviceTransport })}
                className="w-full"
              >
                <option value="ssh">SSH</option>
                <option value="api">API (MikroTik 8728)</option>
              </Select>
            </Field>
            <Field label="Identifiant (username) *">
              <Input value={form.username} onChange={(v) => setForm({ ...form, username: v })} className="w-full" />
            </Field>
            <Field label="Type de secret">
              <Select
                value={secretKind}
                onChange={(v) => setSecretKind(v as "password" | "ssh_key")}
                className="w-full"
              >
                <option value="password">Mot de passe</option>
                <option value="ssh_key">Clé SSH privée (ed25519/ECDSA)</option>
              </Select>
            </Field>
          </div>
          {secretKind === "password" ? (
            <Field label={editing ? "Nouveau mot de passe (laisser vide pour garder l'actuel)" : "Mot de passe *"}>
              <Input
                type="password"
                value={form.password ?? ""}
                onChange={(v) => setForm({ ...form, password: v })}
                className="w-full"
              />
            </Field>
          ) : (
            <Field label={editing ? "Nouvelle clé privée (laisser vide pour garder l'actuelle)" : "Clé privée SSH *"}>
              <textarea
                value={(form as { ssh_key?: string }).ssh_key ?? ""}
                onChange={(e) => setForm({ ...form, ...( { ssh_key: e.target.value } as object ) })}
                rows={5}
                className="w-full rounded-md border border-line-strong bg-surface px-3 py-2 font-mono text-xs outline-none focus:border-accent focus-visible:ring-2 focus-visible:ring-accent/30"
                placeholder={"-----BEGIN OPENSSH PRIVATE KEY-----"}
              />
            </Field>
          )}
          <div className="mt-4 flex justify-end gap-2">
            <Button variant="ghost" onClick={() => { setCreating(false); setEditing(null); }}>
              Annuler
            </Button>
            <Button onClick={save}>{editing ? "Enregistrer" : "Créer"}</Button>
          </div>
        </Modal>
      )}

      {provisionFor && (
        <Modal
          title={`Pousser la config sur ${provisionFor.name}`}
          onClose={() => setProvisionFor(null)}
          wide
        >
          <p className="mb-3 text-sm text-muted">
            Les VLANs et sous-réseaux sélectionnés sont créés sur l&apos;appareil
            ({PLATFORM_LABEL[provisionFor.platform]} via{" "}
            {provisionFor.transport === "api" ? "API" : "SSH"}). Un test de connexion réussi est
            recommandé avant.
          </p>
          <div className="grid gap-4 md:grid-cols-2">
            <div>
              <h3 className="mb-2 text-sm font-medium">VLANs</h3>
              <ul className="max-h-48 space-y-1 overflow-auto rounded-md border border-line p-2 text-sm">
                {(vlans ?? []).map((v) => (
                  <li key={v.id}>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={selVlans.has(v.id)}
                        onChange={() => setSelVlans(toggle(selVlans, v.id))}
                      />
                      <span className="font-mono">{v.vid}</span> {v.name}
                    </label>
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <h3 className="mb-2 text-sm font-medium">Sous-réseaux</h3>
              <ul className="max-h-48 space-y-1 overflow-auto rounded-md border border-line p-2 text-sm">
                {(subnets ?? []).map((s) => (
                  <li key={s.id}>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={selSubnets.has(s.id)}
                        onChange={() => setSelSubnets(toggle(selSubnets, s.id))}
                      />
                      <span className="font-mono">{s.cidr}</span> {s.name ?? ""}
                    </label>
                  </li>
                ))}
              </ul>
            </div>
          </div>
          <div className="mt-4 flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setProvisionFor(null)}>
              Annuler
            </Button>
            <Button onClick={runProvision} disabled={provisioning}>
              {provisioning ? "envoi…" : "Pousser sur l'appareil"}
            </Button>
          </div>
          {provisionResult && (
            <div className="mt-4">
              <Badge tone={provisionResult.ok ? "green" : "red"}>
                {provisionResult.ok ? "provisionnement ok" : "échec partiel"}
              </Badge>
              <ul className="mt-2 max-h-56 space-y-2 overflow-auto text-xs">
                {provisionResult.steps.map((s, i) => (
                  <li key={i} className="rounded-md border border-line bg-canvas p-2">
                    <div className="flex items-center gap-2">
                      <Badge tone={s.ok ? "green" : "red"}>{s.ok ? "ok" : "erreur"}</Badge>
                      <code className="font-mono">{s.command.split("\n")[0]}</code>
                    </div>
                    {s.output && <pre className="mt-1 whitespace-pre-wrap text-muted">{s.output}</pre>}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </Modal>
      )}

      <ConfirmDialog
        open={deleteFor !== null}
        title="Supprimer cet appareil ?"
        message={
          deleteFor
            ? `L'appareil « ${deleteFor.name} » (${deleteFor.host}:${deleteFor.port}) sera supprimé définitivement. Ses secrets chiffrés le seront aussi.`
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
