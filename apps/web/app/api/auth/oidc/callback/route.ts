import { NextRequest, NextResponse } from "next/server";

import { COOKIE_NAME } from "@/lib/server-api";

// Callback OIDC : le navigateur y est redirigé par le fournisseur SSO avec
// ?code&state. L'API échange le code (avec PKCE), vérifie l'id_token et renvoie
// le JWT ; on le pose en cookie HttpOnly puis on redirige vers le tableau de bord.

const API_INTERNAL_URL = process.env.API_INTERNAL_URL ?? "http://localhost:8080";

export async function GET(req: NextRequest) {
  const clientIp =
    req.headers.get("x-forwarded-for") ?? req.headers.get("x-real-ip") ?? undefined;
  const res = await fetch(`${API_INTERNAL_URL}/auth/oidc/callback${req.nextUrl.search}`, {
    method: "GET",
    headers: {
      "Content-Type": "application/json",
      ...(clientIp ? { "x-forwarded-for": clientIp } : {}),
    },
    cache: "no-store",
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    return NextResponse.redirect(new URL("/login?error=sso", req.url), 303);
  }
  const { token, expires_in } = data as { token: string; expires_in: number };
  const response = NextResponse.redirect(new URL("/dashboard", req.url), 303);
  response.cookies.set(COOKIE_NAME, token, {
    httpOnly: true,
    secure: process.env.NODE_ENV === "production",
    sameSite: "lax",
    path: "/",
    maxAge: expires_in,
  });
  return response;
}
