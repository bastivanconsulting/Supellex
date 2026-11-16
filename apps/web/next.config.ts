import type { NextConfig } from "next";

const isDev = process.env.NODE_ENV === "development";

const nextConfig: NextConfig = {
  // Le front ne parle jamais directement à l'API Rust : tout passe par le BFF
  // /api/* qui lit le cookie HttpOnly et forward le JWT côté serveur.
  reactStrictMode: true,
  // Pas de badge "N" fixe en bas à gauche en dev : il recouvrait le pied de
  // la page de login et le bouton de déconnexion.
  devIndicators: false,
  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          { key: "X-DNS-Prefetch-Control", value: "off" },
          { key: "X-Frame-Options", value: "DENY" },
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "Referrer-Policy", value: "no-referrer" },
          {
            key: "Permissions-Policy",
            value: "camera=(), microphone=(), geolocation=()",
          },
          {
            key: "Strict-Transport-Security",
            value: "max-age=63072000; includeSubDomains",
          },
          {
            // Next.js (app router) émet des scripts inline de bootstrap :
            // 'unsafe-inline' est requis pour script-src, tout le reste est
            // verrouillé par défaut (default-src 'self'). En dev, React exige
            // en plus 'unsafe-eval' (uniquement en dev, jamais en prod).
            key: "Content-Security-Policy",
            value: `default-src 'self'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'${isDev ? " 'unsafe-eval'" : ""}`,
          },
        ],
      },
    ];
  },
};

export default nextConfig;
