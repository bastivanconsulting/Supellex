"use client";

import { useEffect, useState } from "react";

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
} from "@/components/ui";
import type { Asset, AssetStatus, AssetType, Location, Paged, Person } from "@supellex/types";

const STATUS_TONE: Record<string, string> = {
  deployed: "green",
  in_stock: "blue",
  repair: "yellow",
  retired: "gray",
  lost: "red",
};

const TYPES: AssetType[] = [
  "laptop", "desktop", "server", "switch", "router", "firewall", "access_point",
  "printer", "phone", "monitor", "nas", "ups", "appliance", "license", "other",
];
const STATUSES: AssetStatus[] = ["in_stock", "deployed", "repair", "retired", "lost"];

interface AssetForm {
  asset_tag: string;
  type: AssetType;
  status: AssetStatus;
  manufacturer: string;
  model: string;
  serial_number: string;
  hostname: string;
  os: string;
  purchase_date: string;
  warranty_end: string;
  purchase_price: string;
  location_id: string;
  assigned_to: string;
  notes: string;
}

const EMPTY: AssetForm = {
  asset_tag: "", type: "laptop", status: "in_stock", manufacturer: "", model: "",
  serial_number: "", hostname: "", os: "", purchase_date: "", warranty_end: "",
  purchase_price: "", location_id: "", assigned_to: "", notes: "",
};

function toForm(a: Asset): AssetForm {
  return {
    asset_tag: a.asset_tag,
    type: a.type,
    status: a.status,
    manufacturer: a.manufacturer ?? "",
    model: a.model ?? "",
    serial_number: a.serial_number ?? "",
    hostname: a.hostname ?? "",
    os: a.os ?? "",
    purchase_date: a.purchase_date ?? "",
    warranty_end: a.warranty_end ?? "",
    purchase_price: a.purchase_price != null ? String(a.purchase_price) : "",
    location_id: a.location_id ?? "",
    assigned_to: a.assigned_to ?? "",
    notes: a.notes ?? "",
  };
}

export default function AssetsPage() {
  const [data, setData] = useState<Paged<Asset> | null>(null);
  const [q, setQ] = useState("");
  const [locations, setLocations] = useState<Location[]>([]);
  const [people, setPeople] = useState<Person[]>([]);
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<Asset | null>(null);
  const [form, setForm] = useState<AssetForm>(EMPTY);
  const [error, setError] = useState<string | null>(null);

  async function reload(search: string) {
    try {
      setData(await api.get<Paged<Asset>>(`/assets?limit=100&q=${encodeURIComponent(search)}`));
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  useEffect(() => {
    const t = setTimeout(() => reload(q), 250);
    return () => clearTimeout(t);
  }, [q]);

  useEffect(() => {
    (async () => {
      try {
        const [lo, pe] = await Promise.all([
          api.get<Location[]>("/locations"),
          api.get<Paged<Person>>("/people?limit=200"),
        ]);
        setLocations(lo);
        setPeople(pe.items);
      } catch {
        /* les listes déroulantes restent vides */
      }
    })();
  }, []);

  async function save() {
    setError(null);
    const body = {
      asset_tag: form.asset_tag,
      type: form.type,
      status: form.status,
      manufacturer: form.manufacturer || null,
      model: form.model || null,
      serial_number: form.serial_number || null,
      hostname: form.hostname || null,
      os: form.os || null,
      purchase_date: form.purchase_date || null,
      warranty_end: form.warranty_end || null,
      purchase_price: form.purchase_price ? Number(form.purchase_price) : null,
      location_id: form.location_id || null,
      assigned_to: form.assigned_to || null,
      notes: form.notes || null,
    };
    try {
      if (creating) await api.post("/assets", body);
      else if (editing) await api.put(`/assets/${editing.id}`, body);
      setCreating(false);
      setEditing(null);
      await reload(q);
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  return (
    <Shell>
      <PageTitle title="Actifs">
        <Button onClick={() => { setForm(EMPTY); setCreating(true); }}>+ Nouvel actif</Button>
      </PageTitle>
      <input
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder="Rechercher (tag, hostname, modèle, n° série)…"
        className="mb-4 w-full max-w-md rounded-md border border-line-strong bg-surface px-3 py-2 text-sm outline-none focus:border-accent"
      />
      {error && <div className="mb-4"><ErrorBox message={error} /></div>}
      {!data ? (
        <Spinner />
      ) : (
        <>
          <p className="mb-2 text-sm text-faint">{data.total} actif(s)</p>
          <Table
            columns={["Tag", "Type", "Statut", "Marque / modèle", "Hostname", "N° série", "OS"]}
            rows={data.items.map((a) => [
              <button
                key={a.id}
                onClick={() => { setForm(toForm(a)); setEditing(a); }}
                className="font-medium text-accent hover:underline"
              >
                {a.asset_tag}
              </button>,
              a.type,
              <Badge key={`${a.id}-s`} tone={STATUS_TONE[a.status]}>{a.status}</Badge>,
              [a.manufacturer, a.model].filter(Boolean).join(" / ") || "—",
              a.hostname ?? "—",
              a.serial_number ?? "—",
              a.os ?? "—",
            ])}
          />
        </>
      )}

      {(creating || editing) && (
        <Modal
          title={creating ? "Nouvel actif" : `Modifier ${editing?.asset_tag}`}
          onClose={() => { setCreating(false); setEditing(null); }}
        >
          <Field label="Étiquette d'inventaire *">
            <Input value={form.asset_tag} onChange={(v) => setForm({ ...form, asset_tag: v })} className="w-full" />
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Type">
              <Select value={form.type} onChange={(v) => setForm({ ...form, type: v as AssetType })} className="w-full">
                {TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
              </Select>
            </Field>
            <Field label="Statut">
              <Select value={form.status} onChange={(v) => setForm({ ...form, status: v as AssetStatus })} className="w-full">
                {STATUSES.map((s) => <option key={s} value={s}>{s}</option>)}
              </Select>
            </Field>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Fabricant">
              <Input value={form.manufacturer} onChange={(v) => setForm({ ...form, manufacturer: v })} className="w-full" />
            </Field>
            <Field label="Modèle">
              <Input value={form.model} onChange={(v) => setForm({ ...form, model: v })} className="w-full" />
            </Field>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <Field label="N° de série">
              <Input value={form.serial_number} onChange={(v) => setForm({ ...form, serial_number: v })} className="w-full" />
            </Field>
            <Field label="Hostname">
              <Input value={form.hostname} onChange={(v) => setForm({ ...form, hostname: v })} className="w-full" />
            </Field>
          </div>
          <Field label="OS">
            <Input value={form.os} onChange={(v) => setForm({ ...form, os: v })} className="w-full" />
          </Field>
          <div className="grid grid-cols-3 gap-3">
            <Field label="Acheté le">
              <Input type="date" value={form.purchase_date} onChange={(v) => setForm({ ...form, purchase_date: v })} className="w-full" />
            </Field>
            <Field label="Garantie jusqu'au">
              <Input type="date" value={form.warranty_end} onChange={(v) => setForm({ ...form, warranty_end: v })} className="w-full" />
            </Field>
            <Field label="Prix (€)">
              <Input type="number" value={form.purchase_price} onChange={(v) => setForm({ ...form, purchase_price: v })} className="w-full" />
            </Field>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Emplacement">
              <Select value={form.location_id} onChange={(v) => setForm({ ...form, location_id: v })} className="w-full">
                <option value="">—</option>
                {locations.map((l) => <option key={l.id} value={l.id}>{l.name}</option>)}
              </Select>
            </Field>
            <Field label="Assigné à">
              <Select value={form.assigned_to} onChange={(v) => setForm({ ...form, assigned_to: v })} className="w-full">
                <option value="">—</option>
                {people.map((p) => (
                  <option key={p.id} value={p.id}>{p.first_name} {p.last_name}</option>
                ))}
              </Select>
            </Field>
          </div>
          <Field label="Notes">
            <Input value={form.notes} onChange={(v) => setForm({ ...form, notes: v })} className="w-full" />
          </Field>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => { setCreating(false); setEditing(null); }}>Annuler</Button>
            <Button onClick={save} disabled={!form.asset_tag.trim()}>
              {creating ? "Créer" : "Enregistrer"}
            </Button>
          </div>
        </Modal>
      )}
    </Shell>
  );
}
