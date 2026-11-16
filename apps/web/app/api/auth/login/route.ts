import { NextRequest, NextResponse } from "next/server";

import { COOKIE_NAME } from "@/lib/server-api";

const API_INTERNAL_URL = process.env.API_INTERNAL_URL ?? "http://localhost:8080";

interface LoginBody {
  email: string;
  password: string;
  source?: string;
}

export async function POST(req: NextRequest) {
  const body = (await req.json()) as LoginBody;
  if (!body.email || !body.password) {
    return NextResponse.json({ error: "email et mot de passe requis" }, { status: 400 });
  }
  const clientIp =
    req.headers.get("x-forwarded-for") ?? req.headers.get("x-real-ip") ?? undefined;
  const res = await fetch(`${API_INTERNAL_URL}/auth/login`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(clientIp ? { "x-forwarded-for": clientIp } : {}),
    },
    body: JSON.stringify({
      email: body.email,
      password: body.password,
      ...(body.source ? { source: body.source } : {}),
    }),
    cache: "no-store",
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    return NextResponse.json(
      { error: (data as { error?: string }).error ?? "identifiants invalides" },
      { status: res.status },
    );
  }
  const { token, expires_in, must_change_password } = data as {
    token: string;
    expires_in: number;
    must_change_password: boolean;
  };
  const response = NextResponse.json({ must_change_password });
  response.cookies.set(COOKIE_NAME, token, {
    httpOnly: true,
    secure: process.env.NODE_ENV === "production",
    sameSite: "lax",
    path: "/",
    maxAge: expires_in,
  });
  return response;
}
