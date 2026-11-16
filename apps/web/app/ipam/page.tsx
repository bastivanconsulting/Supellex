"use client";

import { useCallback, useEffect, useState } from "react";

import { api } from "@/lib/api-client";
import { Shell } from "@/components/sidebar";
import {
  Badge,
  Button,
  ErrorBox,
  Field,
  Input,
  Modal,
  PageTitle,
  Select,
  Spinner,
  Table,
  UtilBar,
} from "@/components/ui";
import type {
  Asset,
  DhcpRange,
  FieldDef,
  IpAddress,
  IpSpace,
  IpStatus,
  Location,
  Paged,
  Person,
  ScanResult,
  Subnet,
  Tag,
  Vlan,
} from "@supellex/types";

const IP_TONE: Record<string, string> = {
  allocated: "green",
  reserved: "blue",
  dhcp: "yellow",
  gateway: "gray",
  offline: "red",
  free: "gray",
};

const IP_STATUSES: IpStatus[] = ["allocated", "reserved", "dhcp", "offline", "free"];

interface IpForm {
  address: string;
  status: IpStatus;
  hostname: string;
  mac: string;
  asset_id: string;
  person_id: string;
  description: string;
  tag_ids: string[];
  custom: Record<string, string>;
}

const EMPTY_FORM: IpForm = {
  address: "",
  status: "allocated",
  hostname: "",
  mac: "",
  asset_id: "",
  person_id: "",
  description: "",
  tag_ids: [],
  custom: {},
};

interface SnForm {
  space_id: string;
  cidr: string;
  name: string;
  description: string;
  gateway: string;
  vlan_id: string;
  location_id: string;
  dns: string;
  is_pool: boolean;
  master_subnet_id: string;
  ping_scan: boolean;
  custom: Record<string, string>;
}

const EMPTY_SN: SnForm = {
  space_id: "",
  cidr: "",
  name: "",
  description: "",
  gateway: "",
  vlan_id: "",
  location_id: "",
  dns: "",
  is_pool: false,
  master_subnet_id: "",
  ping_scan: true,
  custom: {},
};

export default function IpamPage() {
  const [spaces, setSpaces] = useState<IpSpace[] | null>(null);
  const [subnets, setSubnets] = useState<Subnet[] | null>(null);
  const [vlans, setVlans] = useState<Vlan[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [fields, setFields] = useState<FieldDef[]>([]);
  const [assets, setAssets] = useState<Asset[]>([]);
  const [people, setPeople] = useState<Person[]>([]);
  const [spaceId, setSpaceId] = useState<string>("");
  const [selected, setSelected] = useState<Subnet | null>(null);
  const [ips, setIps] = useState<IpAddress[] | null>(null);
  const [children, setChildren] = useState<Subnet[]>([]);
  const [dhcp, setDhcp] = useState<DhcpRange[]>([]);
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [editing, setEditing] = useState<IpAddress | null>(null);
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState<IpForm>(EMPTY_FORM);
  const [error, setError] = useState<string | null>(null);
  const [locations, setLocations] = useState<Location[]>([]);
  const [subnetFields, setSubnetFields] = useState<FieldDef[]>([]);
  const [creatingSpace, setCreatingSpace] = useState(false);
  const [spaceName, setSpaceName] = useState("");
  const [spaceDesc, setSpaceDesc] = useState("");
  const [creatingSubnet, setCreatingSubnet] = useState(false);
  const [snForm, setSnForm] = useState<SnForm>(EMPTY_SN);

  const vlanName = useCallback(
    (id: string | null) => {
      if (!id) return "—";
      const v = vlans.find((x) => x.id === id);
      return v ? `VLAN ${v.vid} ${v.name}` : id.slice(0, 8);
    },
    [vlans],
  );

  const loadSubnet = useCallback(async (id: string) => {
    const [ipPage, kids, ranges] = await Promise.all([
      api.get<Paged<IpAddress>>(`/ipam/subnets/${id}/ips?limit=500`),
      api.get<Subnet[]>(`/ipam/subnets/${id}/children`),
      api.get<DhcpRange[]>(`/ipam/dhcp?subnet_id=${id}`),
    ]);
    setIps(ipPage.items);
    setChildren(kids);
    setDhcp(ranges);
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const [sp, sn, vl, tg, fd, as, pe, lo, sfd] = await Promise.all([
          api.get<IpSpace[]>("/ipam/spaces"),
          api.get<Paged<Subnet>>("/ipam/subnets?limit=200"),
          api.get<Vlan[]>("/ipam/vlans"),
          api.get<Tag[]>("/ipam/tags"),
          api.get<FieldDef[]>("/ipam/fields?entity=ip_address"),
          api.get<Paged<Asset>>("/assets?limit=200"),
          api.get<Paged<Person>>("/people?limit=200"),
          api.get<Location[]>("/locations"),
          api.get<FieldDef[]>("/ipam/fields?entity=subnet"),
        ]);
        setSpaces(sp);
        setSubnets(sn.items);
        setVlans(vl);
        setTags(tg);
        setFields(fd);
        setAssets(as.items);
        setPeople(pe.items);
        setLocations(lo);
        setSubnetFields(sfd);
        if (sp.length > 0) setSpaceId((s) => s || sp[0].id);
      } catch (e) {
        setError(e instanceof Error ? e.message : "erreur");
      }
    })();
  }, []);

  useEffect(() => {
    if (selected) {
      setScan(null);
      loadSubnet(selected.id).catch((e) => setError(e instanceof Error ? e.message : "erreur"));
    }
  }, [selected, loadSubnet]);

  async function reloadSubnets() {
    const sn = await api.get<Paged<Subnet>>("/ipam/subnets?limit=200");
    setSubnets(sn.items);
    return sn.items;
  }

  async function saveSpace() {
    setError(null);
    try {
      const sp = await api.post<IpSpace>("/ipam/spaces", {
        name: spaceName,
        description: spaceDesc || null,
      });
      const all = await api.get<IpSpace[]>("/ipam/spaces");
      setSpaces(all);
      setSpaceId(sp.id);
      setCreatingSpace(false);
      setSpaceName("");
      setSpaceDesc("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  async function saveSubnet() {
    setError(null);
    try {
      const created = await api.post<Subnet>("/ipam/subnets", {
        space_id: snForm.space_id,
        cidr: snForm.cidr,
        name: snForm.name || null,
        description: snForm.description || null,
        gateway: snForm.gateway || null,
        vlan_id: snForm.vlan_id || null,
        location_id: snForm.location_id || null,
        dns_servers: snForm.dns.split(",").map((s) => s.trim()).filter(Boolean),
        is_pool: snForm.is_pool,
        master_subnet_id: snForm.master_subnet_id || null,
        ping_scan: snForm.ping_scan,
        custom_fields: Object.fromEntries(
          Object.entries(snForm.custom).filter(([, v]) => v !== ""),
        ),
      });
      setCreatingSubnet(false);
      const items = await reloadSubnets();
      const fresh = items.find((s) => s.id === created.id) ?? null;
      if (fresh) setSelected(fresh);
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  async function runScan() {
    if (!selected) return;
    setScanning(true);
    setError(null);
    try {
      const res = await api.post<ScanResult>(`/ipam/subnets/${selected.id}/scan`, {});
      setScan(res);
      await loadSubnet(selected.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    } finally {
      setScanning(false);
    }
  }

  async function pingOne(ip: IpAddress) {
    try {
      await api.post(`/ipam/ips/${ip.id}/ping`, {});
      if (selected) await loadSubnet(selected.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  async function openCreate() {
    if (!selected) return;
    try {
      const nf = await api.get<{ address: string }>(
        `/ipam/subnets/${selected.id}/next-free`,
      );
      setForm({ ...EMPTY_FORM, address: nf.address });
      setCreating(true);
    } catch {
      setForm({ ...EMPTY_FORM });
      setCreating(true);
    }
  }

  function openEdit(ip: IpAddress) {
    setForm({
      address: ip.address,
      status: ip.status,
      hostname: ip.hostname ?? "",
      mac: ip.mac ?? "",
      asset_id: ip.asset_id ?? "",
      person_id: ip.person_id ?? "",
      description: ip.description ?? "",
      tag_ids: ip.tags.map((t) => t.id),
      custom: Object.fromEntries(
        Object.entries(ip.custom_fields).map(([k, v]) => [k, String(v)]),
      ),
    });
    setEditing(ip);
  }

  async function saveIp() {
    setError(null);
    const body = {
      status: form.status,
      hostname: form.hostname || null,
      mac: form.mac || null,
      asset_id: form.asset_id || null,
      person_id: form.person_id || null,
      description: form.description || null,
      tag_ids: form.tag_ids,
      custom_fields: Object.fromEntries(
        Object.entries(form.custom).filter(([, v]) => v !== ""),
      ),
      ...(creating ? { address: form.address || undefined } : {}),
    };
    try {
      if (creating && selected) {
        await api.post(`/ipam/subnets/${selected.id}/ips`, body);
      } else if (editing) {
        await api.put(`/ipam/ips/${editing.id}`, body);
      }
      setCreating(false);
      setEditing(null);
      if (selected) await loadSubnet(selected.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  const visible = (subnets ?? []).filter((s) => !spaceId || s.space_id === spaceId);

  return (
    <Shell>
      <PageTitle title="IPAM" />
      {error && <div className="mb-4"><ErrorBox message={error} /></div>}
      {!spaces || !subnets ? (
        <Spinner />
      ) : (
        <>
          <div className="mb-4 flex flex-wrap items-center gap-3">
            <span className="text-sm text-muted">Espace (VRF) :</span>
            <Select value={spaceId} onChange={setSpaceId}>
              {spaces.map((s) => (
                <option key={s.id} value={s.id}>{s.name}</option>
              ))}
            </Select>
            <Button
              variant="ghost"
              onClick={() => { setSpaceName(""); setSpaceDesc(""); setCreatingSpace(true); }}
            >
              + Nouvel espace
            </Button>
            <Button
              onClick={() => {
                setSnForm({ ...EMPTY_SN, space_id: spaceId || (spaces ?? [])[0]?.id || "" });
                setCreatingSubnet(true);
              }}
            >
              + Nouveau sous-réseau
            </Button>
          </div>

          <h2 className="mb-2 text-lg font-medium">Sous-réseaux</h2>
          <Table
            columns={["CIDR", "Nom", "Passerelle", "VLAN", "Utilisation", "Enfants", "Scan ping"]}
            rows={visible.map((s) => [
              <button
                key={s.id}
                onClick={() => setSelected(s)}
                className={`font-mono text-accent hover:underline ${selected?.id === s.id ? "underline font-semibold" : ""}`}
              >
                {s.cidr}
              </button>,
              s.name ?? "—",
              s.gateway?.split("/")[0] ?? "—",
              vlanName(s.vlan_id),
              <span key={`${s.id}-u`}>
                <UtilBar pct={s.utilization_pct} />
                <span className="text-xs text-faint">{s.used_count} / {s.usable_addresses}</span>
              </span>,
              s.children_count > 0 ? String(s.children_count) : "—",
              s.ping_scan ? "auto" : "non",
            ])}
          />

          {selected && (
            <div className="mt-8">
              <div className="mb-3 flex flex-wrap items-center gap-2">
                <h2 className="text-lg font-medium">
                  {selected.cidr} {selected.name ? `\u00A0: ${selected.name}` : ""}
                </h2>
                <Button onClick={runScan} disabled={scanning}>
                  {scanning ? "scan en cours…" : "Scanner le sous-réseau (ping)"}
                </Button>
                <Button variant="ghost" onClick={openCreate}>Allouer une IP</Button>
              </div>
              {scan && (
                <p className="mb-3 text-sm text-muted">
                  Scan : {scan.scanned} adresses testées, {scan.online} en ligne,{" "}
                  {scan.offline} passées hors ligne, {scan.new_ips} découvertes,{" "}
                  {scan.mac_filled} MAC remplies via ARP.
                </p>
              )}

              <div className="mb-4 flex flex-wrap gap-2 text-xs">
                <Badge tone="gray">libres {selected.free_count}</Badge>
                <Badge tone="green">allouées {selected.allocated_count}</Badge>
                <Badge tone="blue">réservées {selected.reserved_count}</Badge>
                <Badge tone="yellow">DHCP {selected.dhcp_count}</Badge>
                <Badge tone="red">hors ligne {selected.offline_count}</Badge>
              </div>

              {children.length > 0 && (
                <div className="mb-4">
                  <h3 className="mb-1 font-medium">Sous-réseaux enfants</h3>
                  <Table
                    columns={["CIDR", "Nom", "Utilisation"]}
                    rows={children.map((c) => [
                      <button
                        key={c.id}
                        onClick={() => setSelected(c)}
                        className="font-mono text-accent hover:underline"
                      >
                        {c.cidr}
                      </button>,
                      c.name ?? "—",
                      <UtilBar key={c.id} pct={c.utilization_pct} />,
                    ])}
                  />
                </div>
              )}

              {dhcp.length > 0 && (
                <div className="mb-4">
                  <h3 className="mb-1 font-medium">Plages DHCP</h3>
                  <Table
                    columns={["Début", "Fin", "Nom"]}
                    rows={dhcp.map((d) => [
                      <span key={d.id} className="font-mono">{d.start_addr}</span>,
                      <span key={`${d.id}-e`} className="font-mono">{d.end_addr}</span>,
                      d.name ?? "—",
                    ])}
                  />
                </div>
              )}

              <h3 className="mb-2 font-medium">Adresses IP</h3>
              {!ips ? (
                <Spinner />
              ) : (
                <Table
                  columns={["Adresse", "Statut", "Ping", "Hostname", "MAC", "Tags", "Vu le", "Actions"]}
                  rows={ips.map((ip) => [
                    <button
                      key={ip.id}
                      onClick={() => openEdit(ip)}
                      className="font-mono text-accent hover:underline"
                    >
                      {ip.address}
                    </button>,
                    <Badge key={`${ip.id}-s`} tone={IP_TONE[ip.status]}>{ip.status}</Badge>,
                    <span key={`${ip.id}-p`} title={ip.last_ping_at ?? ""}>
                      {ip.last_ping_ok === true && <Badge tone="green">en ligne</Badge>}
                      {ip.last_ping_ok === false && <Badge tone="red">hors ligne</Badge>}
                      {ip.last_ping_ok === null && <span className="text-faint">—</span>}
                    </span>,
                    ip.hostname ?? "—",
                    <span key={`${ip.id}-m`} className="font-mono text-xs">{ip.mac ?? "—"}</span>,
                    <span key={`${ip.id}-t`}>
                      {ip.tags.map((t) => (
                        <Badge key={t.id}>{t.name}</Badge>
                      ))}
                    </span>,
                    ip.last_seen ? new Date(ip.last_seen).toLocaleString("fr-FR") : "—",
                    <Button key={`${ip.id}-a`} variant="ghost" onClick={() => pingOne(ip)}>
                      ping
                    </Button>,
                  ])}
                />
              )}
            </div>
          )}
        </>
      )}

      {(creating || editing) && (
        <Modal
          title={creating ? `Allouer une IP dans ${selected?.cidr}` : `Modifier ${editing?.address}`}
          onClose={() => { setCreating(false); setEditing(null); }}
        >
          <Field label="Adresse IP">
            <Input
              value={form.address}
              onChange={(v) => setForm({ ...form, address: v })}
              className="w-full font-mono"
            />
          </Field>
          <Field label="Statut">
            <Select
              value={form.status}
              onChange={(v) => setForm({ ...form, status: v as IpStatus })}
              className="w-full"
            >
              {IP_STATUSES.map((s) => (
                <option key={s} value={s}>{s}</option>
              ))}
            </Select>
          </Field>
          <Field label="Hostname">
            <Input
              value={form.hostname}
              onChange={(v) => setForm({ ...form, hostname: v })}
              className="w-full"
            />
          </Field>
          <Field label="Adresse MAC">
            <Input
              value={form.mac}
              onChange={(v) => setForm({ ...form, mac: v })}
              placeholder="AA:BB:CC:DD:EE:FF"
              className="w-full font-mono"
            />
          </Field>
          <Field label="Actif lié">
            <Select
              value={form.asset_id}
              onChange={(v) => setForm({ ...form, asset_id: v })}
              className="w-full"
            >
              <option value="">—</option>
              {assets.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.asset_tag} {a.hostname ?? ""}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Personne liée">
            <Select
              value={form.person_id}
              onChange={(v) => setForm({ ...form, person_id: v })}
              className="w-full"
            >
              <option value="">—</option>
              {people.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.first_name} {p.last_name}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Description">
            <Input
              value={form.description}
              onChange={(v) => setForm({ ...form, description: v })}
              className="w-full"
            />
          </Field>
          {fields.map((f) => (
            <Field key={f.id} label={`Champ perso : ${f.name}${f.required ? " *" : ""}`}>
              {f.field_type === "select" ? (
                <Select
                  value={form.custom[f.name] ?? ""}
                  onChange={(v) => setForm({ ...form, custom: { ...form.custom, [f.name]: v } })}
                  className="w-full"
                >
                  <option value="">—</option>
                  {f.options.map((o) => (
                    <option key={o} value={o}>{o}</option>
                  ))}
                </Select>
              ) : (
                <Input
                  type={f.field_type === "number" ? "number" : f.field_type === "date" ? "date" : "text"}
                  value={form.custom[f.name] ?? ""}
                  onChange={(v) => setForm({ ...form, custom: { ...form.custom, [f.name]: v } })}
                  className="w-full"
                />
              )}
            </Field>
          ))}
          <div className="mb-4">
            <span className="mb-1 block text-sm text-muted">Tags</span>
            <div className="flex flex-wrap gap-2">
              {tags.map((t) => (
                <label key={t.id} className="flex items-center gap-1 text-sm">
                  <input
                    type="checkbox"
                    checked={form.tag_ids.includes(t.id)}
                    onChange={(e) =>
                      setForm({
                        ...form,
                        tag_ids: e.target.checked
                          ? [...form.tag_ids, t.id]
                          : form.tag_ids.filter((x) => x !== t.id),
                      })
                    }
                  />
                  <Badge>{t.name}</Badge>
                </label>
              ))}
            </div>
          </div>
          <div className="flex justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => { setCreating(false); setEditing(null); }}
            >
              Annuler
            </Button>
            <Button onClick={saveIp}>{creating ? "Allouer" : "Enregistrer"}</Button>
          </div>
        </Modal>
      )}

      {creatingSpace && (
        <Modal title="Nouvel espace (VRF)" onClose={() => setCreatingSpace(false)}>
          <Field label="Nom *">
            <Input value={spaceName} onChange={setSpaceName} className="w-full" />
          </Field>
          <Field label="Description">
            <Input value={spaceDesc} onChange={setSpaceDesc} className="w-full" />
          </Field>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setCreatingSpace(false)}>Annuler</Button>
            <Button onClick={saveSpace} disabled={!spaceName.trim()}>Créer</Button>
          </div>
        </Modal>
      )}

      {creatingSubnet && (
        <Modal title="Nouveau sous-réseau" onClose={() => setCreatingSubnet(false)}>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Espace (VRF) *">
              <Select
                value={snForm.space_id}
                onChange={(v) => setSnForm({ ...snForm, space_id: v })}
                className="w-full"
              >
                {(spaces ?? []).map((s) => (
                  <option key={s.id} value={s.id}>{s.name}</option>
                ))}
              </Select>
            </Field>
            <Field label="CIDR *">
              <Input
                value={snForm.cidr}
                onChange={(v) => setSnForm({ ...snForm, cidr: v })}
                placeholder="192.168.20.0/24"
                className="w-full font-mono"
              />
            </Field>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Nom">
              <Input value={snForm.name} onChange={(v) => setSnForm({ ...snForm, name: v })} className="w-full" />
            </Field>
            <Field label="Passerelle">
              <Input
                value={snForm.gateway}
                onChange={(v) => setSnForm({ ...snForm, gateway: v })}
                placeholder="192.168.20.1"
                className="w-full font-mono"
              />
            </Field>
          </div>
          <Field label="Description">
            <Input value={snForm.description} onChange={(v) => setSnForm({ ...snForm, description: v })} className="w-full" />
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="VLAN">
              <Select value={snForm.vlan_id} onChange={(v) => setSnForm({ ...snForm, vlan_id: v })} className="w-full">
                <option value="">—</option>
                {vlans.map((v) => (
                  <option key={v.id} value={v.id}>VLAN {v.vid} {v.name}</option>
                ))}
              </Select>
            </Field>
            <Field label="Emplacement">
              <Select value={snForm.location_id} onChange={(v) => setSnForm({ ...snForm, location_id: v })} className="w-full">
                <option value="">—</option>
                {locations.map((l) => (
                  <option key={l.id} value={l.id}>{l.name}</option>
                ))}
              </Select>
            </Field>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Sous-réseau parent">
              <Select
                value={snForm.master_subnet_id}
                onChange={(v) => setSnForm({ ...snForm, master_subnet_id: v })}
                className="w-full"
              >
                <option value="">— (racine)</option>
                {(subnets ?? [])
                  .filter((s) => s.space_id === snForm.space_id)
                  .map((s) => (
                    <option key={s.id} value={s.id}>{s.cidr} {s.name ?? ""}</option>
                  ))}
              </Select>
            </Field>
            <Field label="Serveurs DNS (séparés par des virgules)">
              <Input value={snForm.dns} onChange={(v) => setSnForm({ ...snForm, dns: v })} className="w-full font-mono" />
            </Field>
          </div>
          <div className="mb-3 flex gap-4 text-sm">
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={snForm.is_pool}
                onChange={(e) => setSnForm({ ...snForm, is_pool: e.target.checked })}
              />
              plage réservée aux attributions (pool)
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={snForm.ping_scan}
                onChange={(e) => setSnForm({ ...snForm, ping_scan: e.target.checked })}
              />
              balayage ping automatique
            </label>
          </div>
          {subnetFields.map((f) => (
            <Field key={f.id} label={`Champ perso : ${f.name}${f.required ? " *" : ""}`}>
              {f.field_type === "select" ? (
                <Select
                  value={snForm.custom[f.name] ?? ""}
                  onChange={(v) => setSnForm({ ...snForm, custom: { ...snForm.custom, [f.name]: v } })}
                  className="w-full"
                >
                  <option value="">—</option>
                  {f.options.map((o) => <option key={o} value={o}>{o}</option>)}
                </Select>
              ) : (
                <Input
                  type={f.field_type === "number" ? "number" : f.field_type === "date" ? "date" : "text"}
                  value={snForm.custom[f.name] ?? ""}
                  onChange={(v) => setSnForm({ ...snForm, custom: { ...snForm.custom, [f.name]: v } })}
                  className="w-full"
                />
              )}
            </Field>
          ))}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setCreatingSubnet(false)}>Annuler</Button>
            <Button onClick={saveSubnet} disabled={!snForm.cidr.trim() || !snForm.space_id}>
              Créer
            </Button>
          </div>
        </Modal>
      )}
    </Shell>
  );
}
