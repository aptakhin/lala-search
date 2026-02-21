/**
 * Mailtrap sandbox API helpers for intercepting magic-link emails.
 *
 * Ported from conftest.py — uses Playwright's APIRequestContext to call the
 * Mailtrap REST API (not the LalaSearch agent).
 */

import { request as playwrightRequest } from "@playwright/test";
import {
  MAILTRAP_API_TOKEN,
  MAILTRAP_ACCOUNT_ID,
  MAILTRAP_INBOX_ID,
  REQUEST_TIMEOUT,
  EMAIL_WAIT_TIMEOUT,
} from "./config";

function requireMailtrap(): void {
  if (!MAILTRAP_API_TOKEN || !MAILTRAP_ACCOUNT_ID || !MAILTRAP_INBOX_ID) {
    throw new Error(
      "MAILTRAP_API_TOKEN, MAILTRAP_ACCOUNT_ID, and MAILTRAP_INBOX_ID " +
        "must be set to run multi-tenant auth tests.",
    );
  }
}

function messagesUrl(): string {
  return (
    `https://mailtrap.io/api/accounts/` +
    `${MAILTRAP_ACCOUNT_ID}/inboxes/${MAILTRAP_INBOX_ID}/messages`
  );
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Delete all messages in the configured Mailtrap inbox.
 */
export async function clearMailtrapInbox(): Promise<void> {
  requireMailtrap();
  const cleanUrl =
    `https://mailtrap.io/api/accounts/` +
    `${MAILTRAP_ACCOUNT_ID}/inboxes/${MAILTRAP_INBOX_ID}/clean`;

  const ctx = await playwrightRequest.newContext({
    extraHTTPHeaders: { "Api-Token": MAILTRAP_API_TOKEN },
  });
  try {
    const resp = await ctx.patch(cleanUrl, { timeout: REQUEST_TIMEOUT });
    if (resp.status() !== 200 && resp.status() !== 204) {
      console.log(
        `[mailtrap] Warning: inbox clear returned ${resp.status()}`,
      );
    }
  } finally {
    await ctx.dispose();
  }
}

/**
 * Poll the Mailtrap inbox until an email addressed to `toEmail` arrives.
 * Extracts and returns the magic-link token (64-character hex string).
 * Deletes the message after reading to keep the inbox clean.
 */
export async function getMagicLinkToken(
  toEmail: string,
  timeout: number = EMAIL_WAIT_TIMEOUT,
): Promise<string> {
  requireMailtrap();
  const url = messagesUrl();
  const deadline = Date.now() + timeout;

  const ctx = await playwrightRequest.newContext({
    extraHTTPHeaders: { "Api-Token": MAILTRAP_API_TOKEN },
  });

  try {
    while (Date.now() < deadline) {
      const resp = await ctx.get(url, { timeout: REQUEST_TIMEOUT });
      if (resp.status() !== 200) {
        await sleep(2000);
        continue;
      }

      const messages = await resp.json();
      for (const msg of messages) {
        const toField = (msg.to_email || "").toLowerCase();
        if (!toField.includes(toEmail.toLowerCase())) continue;

        const msgId = msg.id;

        // Fetch the plain-text body (more reliable for token extraction)
        let bodyResp = await ctx.get(`${url}/${msgId}/body.txt`, {
          timeout: REQUEST_TIMEOUT,
        });
        let body = bodyResp.status() === 200 ? await bodyResp.text() : "";

        // Fallback to HTML body
        if (!body) {
          bodyResp = await ctx.get(`${url}/${msgId}/body.html`, {
            timeout: REQUEST_TIMEOUT,
          });
          body = bodyResp.status() === 200 ? await bodyResp.text() : "";
        }

        // Extract token from /auth/verify/{64-char hex token}
        const match = body.match(/\/auth\/verify\/([a-f0-9]{64})/);
        if (match) {
          const token = match[1];
          // Delete message to keep inbox tidy
          await ctx.delete(`${url}/${msgId}`, { timeout: REQUEST_TIMEOUT });
          return token;
        }
      }

      await sleep(2000);
    }
  } finally {
    await ctx.dispose();
  }

  throw new Error(
    `Magic-link email to '${toEmail}' not received within ${timeout}ms`,
  );
}
