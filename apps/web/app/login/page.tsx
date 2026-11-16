"use client";

import Image from "next/image";
import { Suspense, useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";

import { api } from "@/lib/api-client";
import { ErrorBox } from "@/components/ui";
import type { OidcStartResponse, PublicAuthProvider } from "@supellex/types";

function LoginInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [providers, setProviders] = useState<PublicAuthProvider[]>([
    { slug: "local", label: "Compte local", kind: "local", login_label: null },
  ]);
  const [source, setSource] = useState("local");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(
    searchParams.get("error") === "sso"
      ? "la connexion via ce fournisseur a échoué, réessaie ou choisis une autre source"
      : null,
  );
  const [loading, setLoading] = useState(false);
  const [ssoLoading, setSsoLoading] = useState<string | null>(null);

  useEffect(() => {
    api
      .get<PublicAuthProvider[]>("/auth/providers")
      .then(setProviders)
      .catch(() => setProviders([]));
  }, []);

  const ldapProviders = providers.filter((p) => p.kind === "ldap");
  const oidcProviders = providers.filter((p) => p.kind === "oidc");
  const selected = providers.find((p) => p.slug === source) ?? providers[0];
  const identLabel =
    selected.kind === "ldap" ? (selected.login_label ?? "Identifiant").toUpperCase() : "E-mail";

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email, password, source }),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) {
        setError((data as { error?: string }).error ?? "identifiants invalides");
        return;
      }
      router.push("/dashboard");
    } catch {
      setError("API injoignable");
    } finally {
      setLoading(false);
    }
  }

  async function startSso(slug: string) {
    setSsoLoading(slug);
    setError(null);
    try {
      const { authorization_url } = await api.get<OidcStartResponse>(`/auth/oidc/${slug}/start`);
      window.location.assign(authorization_url);
    } catch (e) {
      setError(e instanceof Error ? e.message : "fournisseur SSO injoignable");
      setSsoLoading(null);
    }
  }

  return (
    <div className="grid min-h-screen md:grid-cols-[5fr_7fr]">
      {/* Panneau marque, asymétrique : logo en haut, accroche et rappel en bas. */}
      <div className="hidden flex-col justify-between border-r border-line bg-surface p-10 md:flex">
        <Image
          src="/logo-bastivan-light-bg.png"
          alt="Bastivan Consulting"
          width={781}
          height={200}
          className="h-auto w-full max-w-sm"
          priority
          unoptimized
        />
        <div>
          <p className="font-display text-3xl leading-snug">Le parc, sans tableur.</p>
          <p className="mt-4 max-w-sm text-muted">
            Actifs, annuaire, emplacements et IPAM réunis dans un seul outil interne, tenu à jour
            au fil des arrivées et des départs.
          </p>
        </div>
        <p className="text-sm text-faint">
          Outil interne Bastivan Consulting&nbsp;: accès réservé aux comptes habilités.
        </p>
      </div>
      <div className="flex items-center justify-center bg-canvas p-6">
        <form
          onSubmit={submit}
          className="w-full max-w-sm rounded-xl border border-line bg-surface p-8 shadow-sm"
        >
          <div className="mb-6 md:hidden">
            <Image
              src="/logo-bastivan-light-bg.png"
              alt="Bastivan Consulting"
              width={781}
              height={200}
              className="h-auto w-40"
              priority
              unoptimized
            />
          </div>
          <h1 className="mb-6 font-display text-2xl">Connexion</h1>
          {error && (
            <div className="mb-4">
              <ErrorBox message={error} />
            </div>
          )}
          {ldapProviders.length > 0 && (
            <label className="mb-3 block text-sm">
              <span className="mb-1 block text-muted">Source</span>
              <select
                value={source}
                onChange={(e) => setSource(e.target.value)}
                className="w-full rounded-md border border-line-strong bg-surface px-3 py-2 outline-none focus:border-accent focus-visible:ring-2 focus-visible:ring-accent/30"
              >
                {providers
                  .filter((p) => p.kind !== "oidc")
                  .map((p) => (
                    <option key={p.slug} value={p.slug}>
                      {p.label}
                    </option>
                  ))}
              </select>
            </label>
          )}
          <label className="mb-3 block text-sm">
            <span className="mb-1 block text-muted">{identLabel}</span>
            <input
              type={selected.kind === "ldap" ? "text" : "email"}
              required
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              className="w-full rounded-md border border-line-strong bg-surface px-3 py-2 outline-none focus:border-accent focus-visible:ring-2 focus-visible:ring-accent/30"
            />
          </label>
          <label className="mb-6 block text-sm">
            <span className="mb-1 block text-muted">Mot de passe</span>
            <input
              type="password"
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="w-full rounded-md border border-line-strong bg-surface px-3 py-2 outline-none focus:border-accent focus-visible:ring-2 focus-visible:ring-accent/30"
            />
          </label>
          <button
            type="submit"
            disabled={loading}
            className="w-full rounded-md bg-accent py-2 font-medium text-white hover:bg-accent-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 disabled:opacity-50"
          >
            {loading ? "connexion…" : "Se connecter"}
          </button>
          {oidcProviders.length > 0 && (
            <>
              <div className="my-5 flex items-center gap-3 text-sm text-faint">
                <span className="h-px flex-1 bg-line" />
                ou
                <span className="h-px flex-1 bg-line" />
              </div>
              {oidcProviders.map((p) => (
                <button
                  key={p.slug}
                  type="button"
                  onClick={() => startSso(p.slug)}
                  disabled={ssoLoading !== null}
                  className="mb-2 w-full rounded-md border border-line-strong bg-surface py-2 font-medium text-ink hover:bg-canvas focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 disabled:opacity-50"
                >
                  {ssoLoading === p.slug ? "redirection…" : `Continuer avec ${p.label}`}
                </button>
              ))}
            </>
          )}
        </form>
      </div>
    </div>
  );
}

export default function LoginPage() {
  return (
    <Suspense fallback={null}>
      <LoginInner />
    </Suspense>
  );
}
