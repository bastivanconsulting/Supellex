"use client";

import { useEffect, useState } from "react";

import { api } from "@/lib/api-client";
import { Shell } from "@/components/sidebar";
import { Badge, Button, ErrorBox, Field, Input, Modal, PageTitle, Select, Spinner, Table } from "@/components/ui";
import type { Location, LocationKind } from "@supellex/types";

const KINDS: LocationKind[] = ["site", "building", "room", "rack"];

interface LocationForm {
  name: string;
  kind: LocationKind;
  parent_id: string;
  address: string;
  description: string;
}

const EMPTY: LocationForm = {
  name: "", kind: "site", parent_id: "", address: "", description: "",
};

export default function LocationsPage() {
  const [data, setData] = useState<Location[] | null>(null);
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<Location | null>(null);
  const [form, setForm] = useState<LocationForm>(EMPTY);
  const [error, setError] = useState<string | null>(null);

  async function reload() {
    try {
      setData(await api.get<Location[]>("/locations"));
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  useEffect(() => {
    reload();
  }, []);

  async function save() {
    setError(null);
    const body = {
      name: form.name,
      kind: form.kind,
      parent_id: form.parent_id || null,
      address: form.address || null,
      description: form.description || null,
    };
    try {
      if (creating) await api.post("/locations", body);
      else if (editing) await api.put(`/locations/${editing.id}`, body);
      setCreating(false);
      setEditing(null);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  return (
    <Shell>
      <PageTitle title="Emplacements">
        <Button onClick={() => { setForm(EMPTY); setCreating(true); }}>+ Nouvel emplacement</Button>
      </PageTitle>
      {error && <div className="mb-4"><ErrorBox message={error} /></div>}
      {!data ? (
        <Spinner />
      ) : (
        <Table
          columns={["Nom", "Type", "Parent", "Adresse", "Description"]}
          rows={data.map((l) => [
            <button
              key={l.id}
              onClick={() => {
                setForm({
                  name: l.name,
                  kind: l.kind,
                  parent_id: l.parent_id ?? "",
                  address: l.address ?? "",
                  description: l.description ?? "",
                });
                setEditing(l);
              }}
              className="font-medium text-accent hover:underline"
            >
              {l.name}
            </button>,
            <Badge key={`${l.id}-k`}>{l.kind}</Badge>,
            data.find((p) => p.id === l.parent_id)?.name ?? "—",
            l.address ?? "—",
            l.description ?? "—",
          ])}
        />
      )}

      {(creating || editing) && (
        <Modal
          title={creating ? "Nouvel emplacement" : `Modifier ${editing?.name}`}
          onClose={() => { setCreating(false); setEditing(null); }}
        >
          <Field label="Nom *">
            <Input value={form.name} onChange={(v) => setForm({ ...form, name: v })} className="w-full" />
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Type">
              <Select value={form.kind} onChange={(v) => setForm({ ...form, kind: v as LocationKind })} className="w-full">
                {KINDS.map((k) => <option key={k} value={k}>{k}</option>)}
              </Select>
            </Field>
            <Field label="Parent">
              <Select value={form.parent_id} onChange={(v) => setForm({ ...form, parent_id: v })} className="w-full">
                <option value="">— (racine)</option>
                {(data ?? [])
                  .filter((l) => l.id !== editing?.id)
                  .map((l) => <option key={l.id} value={l.id}>{l.name}</option>)}
              </Select>
            </Field>
          </div>
          <Field label="Adresse">
            <Input value={form.address} onChange={(v) => setForm({ ...form, address: v })} className="w-full" />
          </Field>
          <Field label="Description">
            <Input value={form.description} onChange={(v) => setForm({ ...form, description: v })} className="w-full" />
          </Field>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => { setCreating(false); setEditing(null); }}>Annuler</Button>
            <Button onClick={save} disabled={!form.name.trim()}>
              {creating ? "Créer" : "Enregistrer"}
            </Button>
          </div>
        </Modal>
      )}
    </Shell>
  );
}
