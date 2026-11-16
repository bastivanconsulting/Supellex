"use client";

import { useEffect, useState } from "react";

import { api } from "@/lib/api-client";
import { Shell } from "@/components/sidebar";
import {
  Button,
  ErrorBox,
  Field,
  Input,
  PageTitle,
  Select,
  Spinner,
  Table,
  UtilBar,
} from "@/components/ui";
import type { Vlan, VlanDomain } from "@supellex/types";

export default function VlansPage() {
  const [domains, setDomains] = useState<VlanDomain[] | null>(null);
  const [vlans, setVlans] = useState<Vlan[] | null>(null);
  const [selected, setSelected] = useState<Vlan | null>(null);
  const [subnets, setSubnets] = useState<Array<{ id: string; cidr: string; name: string | null }>>([]);
  const [newDomain, setNewDomain] = useState("");
  const [vid, setVid] = useState("");
  const [name, setName] = useState("");
  const [domainId, setDomainId] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function reload() {
    const [d, v] = await Promise.all([
      api.get<VlanDomain[]>("/ipam/vlan-domains"),
      api.get<Vlan[]>("/ipam/vlans"),
    ]);
    setDomains(d);
    setVlans(v);
    if (!domainId && d.length > 0) setDomainId(d[0].id);
  }

  useEffect(() => {
    reload().catch((e) => setError(e instanceof Error ? e.message : "erreur"));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!selected) return;
    api
      .get<Array<{ id: string; cidr: string; name: string | null }>>(
        `/ipam/vlans/${selected.id}/subnets`,
      )
      .then(setSubnets)
      .catch((e) => setError(e instanceof Error ? e.message : "erreur"));
  }, [selected]);

  async function createDomain() {
    setError(null);
    try {
      await api.post("/ipam/vlan-domains", { name: newDomain, description: null });
      setNewDomain("");
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  async function createVlan() {
    setError(null);
    const v = parseInt(vid, 10);
    if (!Number.isInteger(v) || v < 1 || v > 4094) {
      setError("VID invalide (1-4094)");
      return;
    }
    try {
      await api.post("/ipam/vlans", {
        vid: v,
        name,
        description: null,
        vlan_domain_id: domainId || null,
      });
      setVid("");
      setName("");
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : "erreur");
    }
  }

  const domainName = (id: string) =>
    domains?.find((d) => d.id === id)?.name ?? id.slice(0, 8);

  return (
    <Shell>
      <PageTitle title="VLANs" />
      {error && <div className="mb-4"><ErrorBox message={error} /></div>}
      {!domains || !vlans ? (
        <Spinner />
      ) : (
        <div className="grid gap-8 lg:grid-cols-2">
          <div>
            <h2 className="mb-2 text-lg font-medium">Domaines VLAN</h2>
            <Table
              columns={["Nom", "Description"]}
              rows={domains.map((d) => [d.name, d.description ?? "—"])}
            />
            <div className="mt-3 flex gap-2">
              <Input
                value={newDomain}
                onChange={setNewDomain}
                placeholder="Nouveau domaine…"
                className="flex-1"
              />
              <Button onClick={createDomain} disabled={!newDomain.trim()}>
                Ajouter
              </Button>
            </div>

            <h2 className="mb-2 mt-8 text-lg font-medium">Nouveau VLAN</h2>
            <div className="rounded-lg border border-line bg-surface p-4">
              <Field label="VID (1-4094)">
                <Input value={vid} onChange={setVid} className="w-full" />
              </Field>
              <Field label="Nom">
                <Input value={name} onChange={setName} className="w-full" />
              </Field>
              <Field label="Domaine">
                <Select value={domainId} onChange={setDomainId} className="w-full">
                  {domains.map((d) => (
                    <option key={d.id} value={d.id}>{d.name}</option>
                  ))}
                </Select>
              </Field>
              <Button onClick={createVlan} disabled={!vid || !name.trim()}>
                Créer le VLAN
              </Button>
            </div>
          </div>

          <div>
            <h2 className="mb-2 text-lg font-medium">
              VLANs ({vlans.length})
            </h2>
            <Table
              columns={["VID", "Nom", "Domaine", "Sous-réseaux", "IP utilisées", "Utilisation"]}
              rows={vlans.map((v) => {
                // utilisation indicative : IP utilisées rapportées à 254 hôtes/sous-réseau
                const pct =
                  v.subnets_count > 0
                    ? Math.min(100, (v.used_count / Math.max(1, v.subnets_count * 254)) * 100)
                    : 0;
                return [
                  <button
                    key={v.id}
                    onClick={() => setSelected(v)}
                    className={`font-mono text-accent hover:underline ${selected?.id === v.id ? "underline font-semibold" : ""}`}
                  >
                    {v.vid}
                  </button>,
                  v.name,
                  domainName(v.vlan_domain_id),
                  String(v.subnets_count),
                  String(v.used_count),
                  <UtilBar key={`${v.id}-u`} pct={pct} />,
                ];
              })}
            />
            {selected && (
              <div className="mt-4">
                <h3 className="mb-1 font-medium">
                  Sous-réseaux du VLAN {selected.vid}&nbsp;: {selected.name}
                </h3>
                {subnets.length === 0 ? (
                  <p className="text-sm text-muted">aucun sous-réseau rattaché</p>
                ) : (
                  <Table
                    columns={["CIDR", "Nom"]}
                    rows={subnets.map((s) => [
                      <span key={s.id} className="font-mono">{s.cidr}</span>,
                      s.name ?? "—",
                    ])}
                  />
                )}
              </div>
            )}
          </div>
        </div>
      )}
    </Shell>
  );
}
