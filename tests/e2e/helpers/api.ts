/**
 * Shared HTTP API helpers for LalaSearch E2E tests.
 *
 * All functions accept an optional `session` parameter. When provided, the
 * request includes a `Cookie: lala_session=...` header for authentication.
 */

import { APIRequestContext, APIResponse } from "@playwright/test";
import { REQUEST_TIMEOUT } from "./config";

// ── Internal helpers ────────────────────────────────────────────────────

function authHeaders(session?: string): Record<string, string> {
  if (!session) return {};
  return { Cookie: `lala_session=${session}` };
}

// ── Domain management ───────────────────────────────────────────────────

export async function addAllowedDomain(
  request: APIRequestContext,
  domain: string,
  opts: { session?: string; notes?: string } = {},
): Promise<APIResponse> {
  return request.post("/admin/allowed-domains", {
    data: { domain, notes: opts.notes ?? "E2E test domain" },
    headers: authHeaders(opts.session),
    timeout: REQUEST_TIMEOUT,
  });
}

export async function deleteAllowedDomain(
  request: APIRequestContext,
  domain: string,
  session?: string,
): Promise<APIResponse> {
  return request.delete(`/admin/allowed-domains/${domain}`, {
    headers: authHeaders(session),
    timeout: REQUEST_TIMEOUT,
  });
}

export async function listAllowedDomains(
  request: APIRequestContext,
  session?: string,
): Promise<APIResponse> {
  return request.get("/admin/allowed-domains", {
    headers: authHeaders(session),
    timeout: REQUEST_TIMEOUT,
  });
}

// ── Queue ───────────────────────────────────────────────────────────────

export async function addToQueue(
  request: APIRequestContext,
  url: string,
  opts: { session?: string; priority?: number } = {},
): Promise<APIResponse> {
  return request.post("/queue/add", {
    data: { url, priority: opts.priority ?? 1 },
    headers: authHeaders(opts.session),
    timeout: REQUEST_TIMEOUT,
  });
}

// ── Search ──────────────────────────────────────────────────────────────

export async function search(
  request: APIRequestContext,
  query: string,
  opts: { session?: string; limit?: number } = {},
): Promise<APIResponse> {
  return request.post("/search", {
    data: { query, limit: opts.limit ?? 10 },
    headers: authHeaders(opts.session),
    timeout: REQUEST_TIMEOUT,
  });
}

// ── Utilities ───────────────────────────────────────────────────────────

export function uniqueDomain(prefix: string = "e2e"): string {
  return `${prefix}-${Date.now()}.example.invalid`;
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
