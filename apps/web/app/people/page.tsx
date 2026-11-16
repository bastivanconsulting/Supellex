"use client";

import { useEffect, useState } from "react";

import { api } from "@/lib/api-client";
import { Shell } from "@/components/sidebar";
import { Button, ErrorBox, Field, Input, Modal, PageTitle, Spinner, Table } from "@/components/ui";
import type { Paged, Person } from "@supellex/types";

interface PersonForm {
  first_name: string;
  last_name: string;
  email: string;
  phone: string;
  department: string;
  job_title: string;
}

const EMPTY: PersonForm = {
  first_name: "", last_name: "", email: "", phone: "", department: "", job_title: "",
};

export default function PeoplePage() {
  const [data, setData] = useState<Paged<Person> | null>(null);
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<Person | null>(null);
  const [form, setForm] = useState<PersonForm>(EMPTY);
  const [error, setError] = useState<string | null>(null);

  async function reload() {
    try {
      setData(await api.get<Paged<Person>>("/people?limit=200"));
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
      first_name: form.first_name,
      last_name: form.last_name,
      email: form.email || null,
      phone: form.phone || null,
      department: form.department || null,
      job_title: form.job_title || null,
    };
    try {
      if (creating) await api.post("/people", body);
      else if (editing) await api.put(`/people/${editing.id}`, body);
      setCreating(false);
      setEditing(null);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  return (
    <Shell>
      <PageTitle title="Personnes">
        <Button onClick={() => { setForm(EMPTY); setCreating(true); }}>+ Nouvelle personne</Button>
      </PageTitle>
      {error && <div className="mb-4"><ErrorBox message={error} /></div>}
      {!data ? (
        <Spinner />
      ) : (
        <Table
          columns={["Nom", "E-mail", "Téléphone", "Service", "Fonction"]}
          rows={data.items.map((p) => [
            <button
              key={p.id}
              onClick={() => {
                setForm({
                  first_name: p.first_name,
                  last_name: p.last_name,
                  email: p.email ?? "",
                  phone: p.phone ?? "",
                  department: p.department ?? "",
                  job_title: p.job_title ?? "",
                });
                setEditing(p);
              }}
              className="font-medium text-accent hover:underline"
            >
              {p.first_name} {p.last_name}
            </button>,
            p.email ?? "—",
            p.phone ?? "—",
            p.department ?? "—",
            p.job_title ?? "—",
          ])}
        />
      )}

      {(creating || editing) && (
        <Modal
          title={creating ? "Nouvelle personne" : `Modifier ${editing?.first_name} ${editing?.last_name}`}
          onClose={() => { setCreating(false); setEditing(null); }}
        >
          <div className="grid grid-cols-2 gap-3">
            <Field label="Prénom *">
              <Input value={form.first_name} onChange={(v) => setForm({ ...form, first_name: v })} className="w-full" />
            </Field>
            <Field label="Nom *">
              <Input value={form.last_name} onChange={(v) => setForm({ ...form, last_name: v })} className="w-full" />
            </Field>
          </div>
          <Field label="E-mail">
            <Input type="email" value={form.email} onChange={(v) => setForm({ ...form, email: v })} className="w-full" />
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Téléphone">
              <Input value={form.phone} onChange={(v) => setForm({ ...form, phone: v })} className="w-full" />
            </Field>
            <Field label="Service">
              <Input value={form.department} onChange={(v) => setForm({ ...form, department: v })} className="w-full" />
            </Field>
          </div>
          <Field label="Fonction">
            <Input value={form.job_title} onChange={(v) => setForm({ ...form, job_title: v })} className="w-full" />
          </Field>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => { setCreating(false); setEditing(null); }}>Annuler</Button>
            <Button onClick={save} disabled={!form.first_name.trim() || !form.last_name.trim()}>
              {creating ? "Créer" : "Enregistrer"}
            </Button>
          </div>
        </Modal>
      )}
    </Shell>
  );
}
