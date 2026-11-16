import type { Metadata } from "next";
import type { ReactNode } from "react";

import "./globals.css";

export const metadata: Metadata = {
  title: "Supellex · gestion de parc & IPAM · Bastivan Consulting",
  description:
    "Gestionnaire de parc informatique et IPAM interne, Bastivan Consulting, Beauvais",
  // Même favicon officiel que bastivan.consulting (PNG 64x64).
  icons: {
    icon: [{ url: "/favicon.png", type: "image/png", sizes: "64x64" }],
  },
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="fr">
      <body>{children}</body>
    </html>
  );
}
