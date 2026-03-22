/**
 * Local inbox helpers for intercepting magic-link emails during E2E tests.
 *
 * Uses the Mailpit REST API exposed by the test-only Docker service.
 */

import { request as playwrightRequest } from "@playwright/test";
import {
  EMAIL_WAIT_TIMEOUT,
  MAILPIT_API_BASE_URL,
  REQUEST_TIMEOUT,
} from "./config";

type MailpitAddress = {
  Address?: string;
};

type MailpitMessageSummary = {
  ID: string;
  To?: MailpitAddress[];
};

type MailpitMessageListResponse = {
  messages?: MailpitMessageSummary[];
};

type MailpitMessage = {
  ID: string;
  Text?: string;
  HTML?: string;
  To?: MailpitAddress[];
};

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function normalizeEmail(email: string): string {
  return email.trim().toLowerCase();
}

function messageTargetsEmail(message: { To?: MailpitAddress[] }, toEmail: string): boolean {
  const wanted = normalizeEmail(toEmail);
  return (message.To ?? []).some((recipient) =>
    normalizeEmail(recipient.Address ?? "") === wanted,
  );
}

export function extractMagicLinkToken(message: {
  Text?: string;
  HTML?: string;
}): string | null {
  const body = `${message.Text ?? ""}\n${message.HTML ?? ""}`;
  const match = body.match(/\/auth\/verify\/([a-f0-9]{64})/);
  return match ? match[1] : null;
}

async function listMessages(): Promise<MailpitMessageSummary[]> {
  const ctx = await playwrightRequest.newContext();
  try {
    const resp = await ctx.get(`${MAILPIT_API_BASE_URL}/messages`, {
      timeout: REQUEST_TIMEOUT,
    });
    if (resp.status() !== 200) {
      throw new Error(
        `Mailpit list messages failed with HTTP ${resp.status()}: ${await resp.text()}`,
      );
    }

    const payload = (await resp.json()) as MailpitMessageListResponse;
    return payload.messages ?? [];
  } finally {
    await ctx.dispose();
  }
}

async function getMessage(id: string): Promise<MailpitMessage> {
  const ctx = await playwrightRequest.newContext();
  try {
    const resp = await ctx.get(`${MAILPIT_API_BASE_URL}/message/${id}`, {
      timeout: REQUEST_TIMEOUT,
    });
    if (resp.status() !== 200) {
      throw new Error(
        `Mailpit get message failed for ${id} with HTTP ${resp.status()}: ${await resp.text()}`,
      );
    }

    return (await resp.json()) as MailpitMessage;
  } finally {
    await ctx.dispose();
  }
}

async function deleteMessages(ids?: string[]): Promise<void> {
  const ctx = await playwrightRequest.newContext();
  try {
    const resp = await ctx.delete(`${MAILPIT_API_BASE_URL}/messages`, {
      data: ids && ids.length > 0 ? { IDs: ids } : {},
      timeout: REQUEST_TIMEOUT,
    });
    if (resp.status() !== 200) {
      throw new Error(
        `Mailpit delete messages failed with HTTP ${resp.status()}: ${await resp.text()}`,
      );
    }
  } finally {
    await ctx.dispose();
  }
}

/**
 * Delete all messages from the local Mailpit inbox.
 */
export async function clearInbox(): Promise<void> {
  await deleteMessages();
}

/**
 * Poll the local Mailpit inbox until an email addressed to `toEmail` arrives.
 * Extracts and returns the magic-link token (64-character hex string).
 * Deletes the matched message after reading to keep the inbox clean.
 */
export async function getMagicLinkToken(
  toEmail: string,
  timeout: number = EMAIL_WAIT_TIMEOUT,
): Promise<string> {
  const deadline = Date.now() + timeout;

  while (Date.now() < deadline) {
    const messages = await listMessages();
    for (const message of messages) {
      if (!messageTargetsEmail(message, toEmail)) {
        continue;
      }

      const fullMessage = await getMessage(message.ID);
      const token = extractMagicLinkToken(fullMessage);
      if (!token) {
        continue;
      }

      await deleteMessages([message.ID]);
      return token;
    }

    await sleep(2000);
  }

  throw new Error(
    `Magic-link email to '${toEmail}' not received within ${timeout}ms`,
  );
}
