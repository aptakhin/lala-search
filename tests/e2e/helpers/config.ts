/**
 * Shared configuration constants for LalaSearch E2E tests.
 *
 * Timeouts are in milliseconds (Playwright convention).
 */

// Agent URL
export const AGENT_URL =
  process.env.LALA_AGENT_URL || "http://localhost:3000";

// Per-request timeout (ms)
export const REQUEST_TIMEOUT = 10_000;

// How long to wait for a magic-link email to arrive in Mailtrap (ms)
export const EMAIL_WAIT_TIMEOUT = 60_000;

// How long the single-tenant pipeline test polls for crawl+index (ms)
export const TEST_TIMEOUT = 60_000;

// How long the multi-tenant pipeline test polls for crawl+index (ms)
export const MULTI_TENANT_TEST_TIMEOUT = 90_000;

// Interval between search polls (ms)
export const POLL_INTERVAL = 3_000;

// Mailtrap sandbox API credentials
export const MAILTRAP_API_TOKEN = process.env.MAILTRAP_API_TOKEN || "";
export const MAILTRAP_ACCOUNT_ID = process.env.MAILTRAP_ACCOUNT_ID || "";
export const MAILTRAP_INBOX_ID = process.env.MAILTRAP_INBOX_ID || "";

// Multi-tenant test constants
export const TENANT2_INVITE_TOKEN = "e2e-test-tenant2-invite-0001";
export const USER1_EMAIL = "user1@test.e2e";
export const USER2_EMAIL = "user2@test.e2e";
