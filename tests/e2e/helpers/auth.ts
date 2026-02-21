/**
 * Authentication helpers for E2E tests.
 *
 * Ported from conftest.py — provides the full magic-link and invitation
 * acceptance flows, returning the lala_session cookie value.
 */

import { request as playwrightRequest } from "@playwright/test";
import { AGENT_URL, REQUEST_TIMEOUT } from "./config";
import { getMagicLinkToken } from "./mailtrap";

/**
 * Full magic-link authentication flow:
 *   1. POST /auth/request-link  → agent sends email via SMTP → Mailtrap captures it
 *   2. Poll Mailtrap API until email with magic-link token arrives
 *   3. GET /auth/verify/{token} (no redirect follow) → agent sets lala_session cookie
 *   4. Return the session token value
 */
export async function authenticateViaMagicLink(
  email: string,
  agentUrl: string = AGENT_URL,
): Promise<string> {
  const ctx = await playwrightRequest.newContext({ baseURL: agentUrl });
  try {
    // Step 1: Request magic link
    const requestResp = await ctx.post("/auth/request-link", {
      data: { email },
      timeout: REQUEST_TIMEOUT,
    });
    if (!requestResp.ok()) {
      const body = await requestResp.text();
      throw new Error(
        `POST /auth/request-link failed for ${email}: ` +
          `HTTP ${requestResp.status()} — ${body}`,
      );
    }

    // Step 2: Retrieve token from Mailtrap
    const token = await getMagicLinkToken(email);

    // Step 3: Verify token — agent returns 302 with Set-Cookie: lala_session=...
    const verifyResp = await ctx.get(`/auth/verify/${token}`, {
      timeout: REQUEST_TIMEOUT,
      maxRedirects: 0,
    });
    if (verifyResp.status() !== 302 && verifyResp.status() !== 303) {
      const body = await verifyResp.text();
      throw new Error(
        `GET /auth/verify/{token} failed for ${email}: ` +
          `expected 302 or 303, got HTTP ${verifyResp.status()} — ${body}`,
      );
    }

    // Step 4: Extract session cookie from Set-Cookie header
    const setCookie = verifyResp.headers()["set-cookie"] || "";
    const sessionMatch = setCookie.match(/lala_session=([^;]+)/);
    if (!sessionMatch) {
      throw new Error(
        `No lala_session cookie in verify response for ${email}. ` +
          `Set-Cookie: ${setCookie}`,
      );
    }
    return sessionMatch[1];
  } finally {
    await ctx.dispose();
  }
}

/**
 * Accept a pre-seeded organization invitation using its raw (unhashed) token.
 * Returns the session cookie value scoped to the invitation's tenant.
 */
export async function acceptInvitation(
  rawToken: string,
  agentUrl: string = AGENT_URL,
): Promise<string> {
  const ctx = await playwrightRequest.newContext({ baseURL: agentUrl });
  try {
    const resp = await ctx.get(`/auth/invitations/${rawToken}/accept`, {
      timeout: REQUEST_TIMEOUT,
      maxRedirects: 0,
    });
    if (resp.status() !== 302 && resp.status() !== 303) {
      const body = await resp.text();
      throw new Error(
        `GET /auth/invitations/{token}/accept failed: ` +
          `expected 302 or 303, got HTTP ${resp.status()} — ${body}`,
      );
    }

    const setCookie = resp.headers()["set-cookie"] || "";
    const sessionMatch = setCookie.match(/lala_session=([^;]+)/);
    if (!sessionMatch) {
      throw new Error(
        `No lala_session cookie in invitation accept response. ` +
          `Set-Cookie: ${setCookie}`,
      );
    }
    return sessionMatch[1];
  } finally {
    await ctx.dispose();
  }
}
