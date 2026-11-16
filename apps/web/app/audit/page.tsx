"use client";

import { useEffect, useState } from "react";

import { api } from "@/lib/api-client";
import { Shell } from "@/components/sidebar";
import { ErrorBox, PageTitle, Spinner, Table } from "@/components/ui";
import type { AuditEntry, Paged } from "@supellex/types";

export default function AuditPage() {
  const [data, setData] = useState<Paged<AuditEntry> | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .get<Paged<AuditEntry>>("/audit?limit=100")
      .then(setData)
      .catch((e) => setError(e instanceof Error ? e.message : "erreur"));
  }, []);

  return (
    <Shell>
      <PageTitle title="Journal d'audit" />
      {error && <ErrorBox message={error} />}
      {!data ? (
        <Spinner />
      ) : (
        <Table
          columns={["Date", "Acteur", "Action", "Entité", "IP"]}
          rows={data.items.map((a) => [
            new Date(a.at).toLocaleString("fr-FR"),
            a.actor_email ?? "—",
            <span key={a.id} className="font-mono text-xs">{a.action}</span>,
            `${a.entity_type}${a.entity_id ? ` (${a.entity_id.slice(0, 8)}…)` : ""}`,
            a.ip ?? "—",
          ])}
        />
      )}
    </Shell>
  );
}
