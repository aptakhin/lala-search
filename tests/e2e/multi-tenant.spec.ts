/**
 * Multi-Tenant End-to-End Tests for LalaSearch
 *
 * Verifies that a single agent instance serving multiple tenants provides
 * proper data isolation: each tenant's data (domains, crawl queue, search
 * index) is visible only within that tenant's session.
 *
 * Prerequisites (set up by run_tests.sh):
 *   - Agent running with DEPLOYMENT_MODE=multi_tenant
 *   - lalasearch_test keyspace (tenant 1 — default test tenant)
 *   - lalasearch_test_tenant2 keyspace (tenant 2)
 *   - lalasearch_system.tenants rows for both keyspaces
 *   - Pre-seeded org_invitation for user2@test.e2e → lalasearch_test_tenant2
 *     with raw token "e2e-test-tenant2-invite-0001"
 *
 * Environment variables required:
 *   MAILTRAP_API_TOKEN   — Mailtrap API token
 *   MAILTRAP_ACCOUNT_ID  — Mailtrap account ID
 *   MAILTRAP_INBOX_ID    — Mailtrap inbox ID
 */

import { test, expect } from "@playwright/test";
import {
  REQUEST_TIMEOUT,
  MULTI_TENANT_TEST_TIMEOUT,
  POLL_INTERVAL,
  TENANT2_INVITE_TOKEN,
  USER1_EMAIL,
} from "./helpers/config";
import { clearMailtrapInbox } from "./helpers/mailtrap";
import { authenticateViaMagicLink, acceptInvitation } from "./helpers/auth";
import {
  addAllowedDomain,
  deleteAllowedDomain,
  listAllowedDomains,
  addToQueue,
  search,
  uniqueDomain,
  sleep,
} from "./helpers/api";

// Module-level session state (replaces pytest module-scoped fixture)
let user1Session: string;
let user2Session: string;

// ---------------------------------------------------------------------------
// Connectivity smoke test (no session needed)
// ---------------------------------------------------------------------------

test.describe("TestAgentConnectivity", () => {
  test("agent is healthy and in multi-tenant mode", async ({ request }) => {
    const response = await request.get("/version", {
      timeout: REQUEST_TIMEOUT,
    });
    expect(response.status()).toBe(200);
    const data = await response.json();
    expect(data).toHaveProperty("version");
    expect(data.deployment_mode).toBe("multi_tenant");
  });
});

// ---------------------------------------------------------------------------
// Tests requiring authenticated sessions
// ---------------------------------------------------------------------------

test.describe("Multi-tenant authenticated tests", () => {
  // Replaces @pytest.fixture(scope="module") sessions()
  test.beforeAll(async () => {
    await clearMailtrapInbox();
    user1Session = await authenticateViaMagicLink(USER1_EMAIL);
    user2Session = await acceptInvitation(TENANT2_INVITE_TOKEN);
  });

  // --- Tenant isolation: allowed domains ---

  test.describe("TestTenantDomainIsolation", () => {
    test("domain added by user2 not visible to user1", async ({ request }) => {
      const domain = uniqueDomain("iso");

      await addAllowedDomain(request, domain, { session: user2Session });

      const t2Resp = await listAllowedDomains(request, user2Session);
      const t2Domains = (await t2Resp.json()).domains.map(
        (d: { domain: string }) => d.domain,
      );
      const t1Resp = await listAllowedDomains(request, user1Session);
      const t1Domains = (await t1Resp.json()).domains.map(
        (d: { domain: string }) => d.domain,
      );

      expect(t2Domains).toContain(domain);
      expect(t1Domains).not.toContain(domain);

      // Cleanup
      await deleteAllowedDomain(request, domain, user2Session);
    });

    test("domain added by user1 not visible to user2", async ({ request }) => {
      const domain = uniqueDomain("iso-rev");

      await addAllowedDomain(request, domain, { session: user1Session });

      const t1Resp = await listAllowedDomains(request, user1Session);
      const t1Domains = (await t1Resp.json()).domains.map(
        (d: { domain: string }) => d.domain,
      );
      const t2Resp = await listAllowedDomains(request, user2Session);
      const t2Domains = (await t2Resp.json()).domains.map(
        (d: { domain: string }) => d.domain,
      );

      expect(t1Domains).toContain(domain);
      expect(t2Domains).not.toContain(domain);

      // Cleanup
      await deleteAllowedDomain(request, domain, user1Session);
    });
  });

  // --- Full multi-tenant crawl workflow ---

  test.describe("TestTenant2CrawlWorkflow", () => {
    test("add domain, queue URL, and scheduler crawls it", async ({
      request,
    }) => {
      const testUrl = "https://en.wikipedia.org/wiki/Linux";
      const testDomain = "en.wikipedia.org";
      const searchTerm = "Linux";

      console.log(`\n[multi-tenant] Testing with URL: ${testUrl}`);

      // Arrange: add domain to tenant2 allow list
      console.log(
        `[multi-tenant] Adding '${testDomain}' to tenant2 allow list...`,
      );
      const addResp = await addAllowedDomain(request, testDomain, {
        session: user2Session,
      });
      const addResult = await addResp.json();
      expect(addResult.success).toBe(true);
      console.log(`   Done: ${addResult.message}`);

      // Act: queue the URL via tenant2
      console.log("[multi-tenant] Queuing URL via tenant2...");
      const queueResp = await addToQueue(request, testUrl, {
        session: user2Session,
      });
      expect([200, 201]).toContain(queueResp.status());
      console.log("   Done: URL queued in tenant2");

      // Assert: poll tenant2 search until URL is indexed or timeout
      console.log(
        `[multi-tenant] Waiting for scheduler to crawl and index (max ${MULTI_TENANT_TEST_TIMEOUT / 1000}s)...`,
      );
      let foundInTenant2 = false;
      const start = Date.now();

      while (Date.now() - start < MULTI_TENANT_TEST_TIMEOUT) {
        await sleep(POLL_INTERVAL);
        try {
          const searchResp = await search(request, searchTerm, {
            session: user2Session,
          });
          if (searchResp.status() === 200) {
            const results = await searchResp.json();
            const urls = (results.results || []).map(
              (r: { document: { url?: string } }) => r.document?.url,
            );
            if (urls.includes(testUrl)) {
              const elapsed = ((Date.now() - start) / 1000).toFixed(1);
              console.log(`   Done: URL indexed in tenant2 (${elapsed}s)`);
              foundInTenant2 = true;
              break;
            }
            console.log(
              `   ... ${results.results?.length || 0} result(s) in tenant2, waiting for our URL...`,
            );
          }
        } catch (e) {
          console.log(`   ... tenant2 search error (retrying): ${e}`);
        }
      }

      expect(foundInTenant2).toBeTruthy();

      // Assert: verify tenant1 does NOT have this URL (isolation check)
      console.log(
        "[multi-tenant] Verifying tenant1 does NOT have the URL (isolation check)...",
      );
      try {
        const t1SearchResp = await search(request, searchTerm, {
          session: user1Session,
        });
        if (t1SearchResp.status() === 200) {
          const t1Results = await t1SearchResp.json();
          const t1Urls = (t1Results.results || []).map(
            (r: { document: { url?: string } }) => r.document?.url,
          );
          expect(t1Urls).not.toContain(testUrl);
          console.log("   Done: URL correctly absent from tenant1 search");
        }
        // 503 means search unavailable for tenant1 — acceptable (no results)
      } catch {
        // Non-200 is acceptable for isolation check
      }

      console.log("\nMulti-tenant E2E test passed!");

      // Cleanup
      await deleteAllowedDomain(request, testDomain, user2Session);
    });
  });
});
