import "server-only";

import { cookies } from "next/headers";

// Appels serveur -> API Rust. Le token lu depuis le cookie HttpOnly est
// forwardé en Authorization ; il ne transite jamais par le navigateur.

export const COOKIE_NAME = "supellex_token";

const API_INTERNAL_URL =
  process.env.API_INTERNAL_URL ?? "http://localhost:8080";

export async function apiFetch(
  path: string,
  init?: { method?: string; body?: unknown; search?: string; clientIp?: string },
): Promise<Response> {
  const store = await cookies();
  const token = store.get(COOKIE_NAME)?.value;
  const url = `${API_INTERNAL_URL}${path}${init?.search ?? ""}`;
  return fetch(url, {
    method: init?.method ?? "GET",
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      // IP réelle du navigateur, pour le rate limiting et l'audit côté API.
      // L'API n'est joignable que par ce BFF, le header est donc de confiance.
      ...(init?.clientIp ? { "x-forwarded-for": init.clientIp } : {}),
    },
    body: init?.body !== undefined ? JSON.stringify(init.body) : undefined,
    // Pas de cache sur des données d'inventaire
    cache: "no-store",
  });
}
