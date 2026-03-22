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

// How long to wait for a magic-link email to arrive in Mailpit (ms)
export const EMAIL_WAIT_TIMEOUT = 60_000;

// How long the single-tenant pipeline test polls for crawl+index (ms)
export const TEST_TIMEOUT = 60_000;

// How long the multi-tenant pipeline test polls for crawl+index (ms)
export const MULTI_TENANT_TEST_TIMEOUT = 90_000;

// Interval between search polls (ms)
export const POLL_INTERVAL = 3_000;

// Mailpit API base URL
export const MAILPIT_API_BASE_URL =
  process.env.MAILPIT_API_BASE_URL || "http://localhost:8025/api/v1";

function sanitizeRunId(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9-]/g, "").slice(0, 24) || "local";
}

const E2E_RUN_ID = sanitizeRunId(
  process.env.E2E_RUN_ID || `${Date.now().toString(36)}`,
);

// Multi-tenant test user emails
export const USER1_EMAIL = `user1-${E2E_RUN_ID}@test.e2e`;
export const USER2_EMAIL = `user2-${E2E_RUN_ID}@test.e2e`;
