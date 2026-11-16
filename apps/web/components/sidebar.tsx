"use client";

import Image from "next/image";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import type { ReactNode } from "react";

import { Badge, Icon } from "@/components/ui";

const NAV = [
  { href: "/dashboard", label: "Tableau de bord", icon: "dashboard" },
  { href: "/assets", label: "Actifs", icon: "assets" },
  { href: "/ipam", label: "IPAM", icon: "ipam" },
  { href: "/vlans", label: "VLANs", icon: "vlans" },
  { href: "/people", label: "Personnes", icon: "people" },
  { href: "/locations", label: "Emplacements", icon: "locations" },
  { href: "/devices", label: "Appareils", icon: "device" },
  { href: "/audit", label: "Audit", icon: "audit" },
  { href: "/settings/auth", label: "Authentification", icon: "key" },
] as const;

export function Sidebar() {
  const pathname = usePathname();
  const router = useRouter();

  async function logout() {
    await fetch("/api/auth/logout", { method: "POST" });
    router.push("/login");
  }

  return (
    <aside className="flex w-60 flex-col border-r border-line bg-surface p-4">
      <Link href="/dashboard" className="mb-6 block">
        {/* Bannière panoramique affichée en pleine largeur du panneau.
            unoptimized : le PNG officiel est servi octet pour octet, jamais
            réencodé par l'optimiseur (pas de perte q=75 sur un asset de marque). */}
        <Image
          src="/logo-bastivan-light-bg.png"
          alt="Bastivan Consulting"
          width={781}
          height={200}
          className="h-auto w-full"
          priority
          unoptimized
        />
        <div className="mt-2 flex items-center gap-2 text-xs text-faint">
          Supellex · gestion de parc &amp; IPAM
          <Badge tone="yellow">bêta</Badge>
        </div>
      </Link>
      <nav className="flex flex-col gap-1">
        {NAV.map((item) => (
          <Link
            key={item.href}
            href={item.href}
            className={`flex items-center gap-3 rounded-md px-3 py-2 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 ${
              pathname.startsWith(item.href)
                ? "bg-accent/10 font-medium text-accent"
                : "text-muted hover:bg-canvas hover:text-ink"
            }`}
          >
            <Icon name={item.icon} className="h-[18px] w-[18px] shrink-0" />
            {item.label}
          </Link>
        ))}
      </nav>
      <button
        onClick={logout}
        className="mt-auto flex items-center gap-3 rounded-md px-3 py-2 text-left text-sm text-faint hover:bg-canvas hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
      >
        <Icon name="logout" className="h-[18px] w-[18px] shrink-0" />
        Déconnexion
      </button>
    </aside>
  );
}

export function Shell({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-screen">
      <Sidebar />
      {/* Colonne de contenu bornée : lignes de 60 à 75 caractères, alignées à gauche. */}
      <main className="min-w-0 flex-1 p-8">
        <div className="max-w-6xl">{children}</div>
      </main>
    </div>
  );
}
