"use client";

import { useEffect, useState } from "react";

import { api } from "@/lib/api-client";
import { Shell } from "@/components/sidebar";
import { Badge, ErrorBox, Icon, Input, PageTitle, Spinner } from "@/components/ui";
import type { Asset, Paged, SearchHit, Subnet, Vlan } from "@supellex/types";

const KIND_LABEL: Record<string, string> = {
  ip: "IP",
  subnet: "Sous-réseau",
  asset: "Actif",
  person: "Personne",
  vlan: "VLAN",
};

const KIND_TONE: Record<string, string> = {
  ip: "blue",
  subnet: "gray",
  asset: "green",
  person: "yellow",
  vlan: "red",
};

export default function DashboardPage() {
  const [me, setMe] = useState<{ email: string; role: string } | null>(null);
  const [assets, setAssets] = useState<Paged<Asset> | null>(null);
  const [subnets, setSubnets] = useState<Paged<Subnet> | null>(null);
  const [vlans, setVlans] = useState<Vlan[] | null>(null);
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const [meRes, assetsRes, subnetsRes, vlansRes] = await Promise.all([
          api.get<{ email: string; role: string }>("/auth/me"),
          api.get<Paged<Asset>>("/assets?limit=1"),
          api.get<Paged<Subnet>>("/ipam/subnets?limit=1"),
          api.get<Vlan[]>("/ipam/vlans"),
        ]);
        setMe(meRes);
        setAssets(assetsRes);
        setSubnets(subnetsRes);
        setVlans(vlansRes);
      } catch (e) {
        setError(e instanceof Error ? e.message : "erreur");
      }
    })();
  }, []);

  useEffect(() => {
    if (q.trim().length < 2) {
      setHits(null);
      return;
    }
    const t = setTimeout(async () => {
      try {
        setHits(await api.get<SearchHit[]>(`/search?q=${encodeURIComponent(q)}&limit=15`));
      } catch (e) {
        setError(e instanceof Error ? e.message : "erreur");
      }
    }, 250);
    return () => clearTimeout(t);
  }, [q]);

  return (
    <Shell>
      <PageTitle title="Tableau de bord" />
      {error && <ErrorBox message={error} />}
      <div className="mb-8 max-w-md">
        <div className="relative">
          <Icon
            name="search"
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-faint"
          />
          <Input
            value={q}
            onChange={setQ}
            placeholder="Recherche globale (IP, MAC, hostname, actif, personne, VLAN)…"
            className="w-full pl-9"
          />
        </div>
      </div>
      {!me || !assets || !subnets || !vlans ? (
        <Spinner />
      ) : (
        <div className="grid gap-10 lg:grid-cols-[minmax(0,1fr)_280px]">
          <section>
            {hits ? (
              <div className="max-w-2xl rounded-lg border border-line bg-surface p-3">
                {hits.length === 0 ? (
                  <p className="text-sm text-muted">aucun résultat</p>
                ) : (
                  <ul className="divide-y divide-line/70">
                    {hits.map((h) => (
                      <li
                        key={`${h.kind}-${h.id}`}
                        className="flex items-center gap-3 py-1.5 text-sm"
                      >
                        <Badge tone={KIND_TONE[h.kind]}>{KIND_LABEL[h.kind] ?? h.kind}</Badge>
                        <span className="font-mono text-ink">{h.label}</span>
                        <span className="truncate text-muted">{h.detail}</span>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            ) : (
              <p className="max-w-md text-muted">
                Lance une recherche ou navigue dans les sections du menu&nbsp;: actifs, IPAM,
                VLANs, personnes, emplacements, audit.
              </p>
            )}
          </section>
          {/* Rail de chiffres, asymétrique : liste chiffrée, pas de cartes identiques. */}
          <aside className="border-t border-line pt-6 lg:border-l lg:border-t-0 lg:pl-8 lg:pt-0">
            <dl className="divide-y divide-line/70">
              <div className="pb-5">
                <dt className="text-sm text-muted">Connecté</dt>
                <dd className="mt-1 truncate font-medium">{me.email}</dd>
                <dd className="text-sm text-faint">rôle&nbsp;: {me.role}</dd>
              </div>
              <div className="py-5">
                <dt className="text-sm text-muted">Actifs recensés</dt>
                <dd className="mt-1 font-display text-4xl tabular-nums">{assets.total}</dd>
              </div>
              <div className="py-5">
                <dt className="text-sm text-muted">Sous-réseaux</dt>
                <dd className="mt-1 font-display text-4xl tabular-nums">{subnets.total}</dd>
              </div>
              <div className="pt-5">
                <dt className="text-sm text-muted">VLANs</dt>
                <dd className="mt-1 font-display text-4xl tabular-nums">{vlans.length}</dd>
              </div>
            </dl>
          </aside>
        </div>
      )}
    </Shell>
  );
}
