import { NextRequest, NextResponse } from "next/server";

import { apiFetch } from "@/lib/server-api";

// BFF générique : /api/<path...> -> API Rust /<path...>
// Le navigateur appelle uniquement /api/* (même origine, cookie HttpOnly).

type Ctx = { params: Promise<{ path: string[] }> };

async function proxy(req: NextRequest, ctx: Ctx, method: string) {
  const { path } = await ctx.params;
  const search = req.nextUrl.search;
  const body =
    method === "GET" || method === "DELETE" ? undefined : await req.json().catch(() => undefined);
  const clientIp =
    req.headers.get("x-forwarded-for") ?? req.headers.get("x-real-ip") ?? undefined;
  const res = await apiFetch(`/${path.join("/")}`, { method, body, search, clientIp });
  // 204/205/304 : le constructeur Response exige un corps null sur ces statuts.
  if (res.status === 204 || res.status === 205 || res.status === 304) {
    return new NextResponse(null, { status: res.status });
  }
  const text = await res.text();
  return new NextResponse(text, {
    status: res.status,
    headers: { "Content-Type": res.headers.get("Content-Type") ?? "application/json" },
  });
}

export async function GET(req: NextRequest, ctx: Ctx) {
  return proxy(req, ctx, "GET");
}
export async function POST(req: NextRequest, ctx: Ctx) {
  return proxy(req, ctx, "POST");
}
export async function PUT(req: NextRequest, ctx: Ctx) {
  return proxy(req, ctx, "PUT");
}
export async function PATCH(req: NextRequest, ctx: Ctx) {
  return proxy(req, ctx, "PATCH");
}
export async function DELETE(req: NextRequest, ctx: Ctx) {
  return proxy(req, ctx, "DELETE");
}
