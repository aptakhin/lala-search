/**
 * End-to-End System Tests for LalaSearch
 *
 * Tests the full HTTP API surface against a live agent.
 * Uses only public APIs, no internal state inspection.
 *
 * Test groups (AAA structure within each test):
 *   - TestVersion          — GET /version
 *   - TestAdminDomains     — POST / GET / DELETE /admin/allowed-domains
 *   - TestQueueEndpoint    — POST /queue/add
 *   - TestCrawlingSettings — GET / PUT /admin/settings/crawling-enabled
 *   - TestSearchEndpoint   — POST /search
 *   - TestFullPipeline     — End-to-end: queue → crawl → index → search
 */

import { test, expect } from "@playwright/test";
import { REQUEST_TIMEOUT, TEST_TIMEOUT } from "./helpers/config";
import {
  addAllowedDomain,
  deleteAllowedDomain,
  listAllowedDomains,
  addToQueue,
  search,
  uniqueDomain,
  sleep,
} from "./helpers/api";

// ---------------------------------------------------------------------------
// GET /version
// ---------------------------------------------------------------------------

test.describe("TestVersion", () => {
  test("returns 200 with version fields", async ({ request }) => {
    const response = await request.get("/version", {
      timeout: REQUEST_TIMEOUT,
    });

    expect(response.status()).toBe(200);
    const data = await response.json();
    expect(data).toHaveProperty("version");
    expect(data).toHaveProperty("agent");
    expect(data).toHaveProperty("deployment_mode");
    expect(data.agent).toBe("lala-agent");
  });

  test("version follows semver", async ({ request }) => {
    const response = await request.get("/version", {
      timeout: REQUEST_TIMEOUT,
    });

    const data = await response.json();
    const parts = data.version.split(".");
    expect(parts).toHaveLength(3);
    for (const p of parts) {
      expect(Number.isInteger(Number(p))).toBeTruthy();
    }
  });

  test("deployment mode is valid", async ({ request }) => {
    const response = await request.get("/version", {
      timeout: REQUEST_TIMEOUT,
    });

    const data = await response.json();
    expect(["single_tenant", "multi_tenant"]).toContain(data.deployment_mode);
  });

  test("unknown route returns 404", async ({ request }) => {
    const response = await request.get("/does-not-exist", {
      timeout: REQUEST_TIMEOUT,
    });

    expect(response.status()).toBe(404);
  });
});

// ---------------------------------------------------------------------------
// POST / GET / DELETE /admin/allowed-domains
// ---------------------------------------------------------------------------

test.describe("TestAdminDomains", () => {
  test("add domain success", async ({ request }) => {
    const domain = uniqueDomain("add");

    const resp = await addAllowedDomain(request, domain, { notes: "add test" });
    expect(resp.ok()).toBeTruthy();
    const result = await resp.json();

    expect(result.success).toBe(true);
    expect(result.domain).toBe(domain);
    expect(result.message).toContain("Domain added");

    // Cleanup
    await deleteAllowedDomain(request, domain);
  });

  test("list domains returns array", async ({ request }) => {
    const resp = await listAllowedDomains(request);
    const result = await resp.json();

    expect(result).toHaveProperty("domains");
    expect(result).toHaveProperty("count");
    expect(result.count).toBe(result.domains.length);
  });

  test("add then list shows domain", async ({ request }) => {
    const domain = uniqueDomain("list");
    await addAllowedDomain(request, domain, { notes: "list test" });

    const resp = await listAllowedDomains(request);
    const result = await resp.json();
    const found = result.domains.find(
      (d: { domain: string }) => d.domain === domain,
    );

    expect(found).toBeTruthy();
    expect(found.notes).toBe("list test");
    expect(found.added_by).not.toBeNull();

    // Cleanup
    await deleteAllowedDomain(request, domain);
  });

  test("delete domain removes it from list", async ({ request }) => {
    const domain = uniqueDomain("del");
    await addAllowedDomain(request, domain);

    await deleteAllowedDomain(request, domain);
    const resp = await listAllowedDomains(request);
    const result = await resp.json();
    const domainNames = result.domains.map(
      (d: { domain: string }) => d.domain,
    );

    expect(domainNames).not.toContain(domain);
  });

  test("delete nonexistent domain is idempotent", async ({ request }) => {
    const domain = uniqueDomain("ghost");

    const resp = await deleteAllowedDomain(request, domain);
    const result = await resp.json();

    expect(result.success).toBe(true);
  });

  test("add empty domain returns 400", async ({ request }) => {
    const response = await request.post("/admin/allowed-domains", {
      data: { domain: "" },
      timeout: REQUEST_TIMEOUT,
    });

    expect(response.status()).toBe(400);
    const text = await response.text();
    expect(text).toContain("Domain cannot be empty");
  });
});

// ---------------------------------------------------------------------------
// POST /queue/add
// ---------------------------------------------------------------------------

test.describe("TestQueueEndpoint", () => {
  test("add approved domain URL succeeds", async ({ request }) => {
    const domain = uniqueDomain("queue");
    const testUrl = `https://${domain}/page`;
    await addAllowedDomain(request, domain);

    const response = await addToQueue(request, testUrl);

    expect(response.status()).toBe(200);
    const data = await response.json();
    expect(data.success).toBe(true);
    expect(data.url).toBe(testUrl);
    expect(data.domain).toBe(domain);

    // Cleanup
    await deleteAllowedDomain(request, domain);
  });

  test("invalid URL returns 400", async ({ request }) => {
    const response = await addToQueue(request, "not-a-valid-url");

    expect(response.status()).toBe(400);
  });

  test("unapproved domain returns 403", async ({ request }) => {
    const unapproved = uniqueDomain("forbidden");
    const response = await addToQueue(request, `https://${unapproved}/page`);

    expect(response.status()).toBe(403);
    const text = await response.text();
    expect(text).toContain("not in the allowed domains list");
  });
});

// ---------------------------------------------------------------------------
// GET / PUT /admin/settings/crawling-enabled
// ---------------------------------------------------------------------------

test.describe("TestCrawlingSettings", () => {
  test("get returns boolean", async ({ request }) => {
    const response = await request.get("/admin/settings/crawling-enabled", {
      timeout: REQUEST_TIMEOUT,
    });

    expect(response.status()).toBe(200);
    const data = await response.json();
    expect(data).toHaveProperty("enabled");
    expect(typeof data.enabled).toBe("boolean");
  });

  test("disable then enable persists", async ({ request }) => {
    const settingsUrl = "/admin/settings/crawling-enabled";
    const originalResp = await request.get(settingsUrl, {
      timeout: REQUEST_TIMEOUT,
    });
    const original = (await originalResp.json()).enabled;

    // Disable
    let r = await request.put(settingsUrl, {
      data: { enabled: false },
      timeout: REQUEST_TIMEOUT,
    });
    expect(r.status()).toBe(200);
    expect((await r.json()).enabled).toBe(false);
    let check = await request.get(settingsUrl, { timeout: REQUEST_TIMEOUT });
    expect((await check.json()).enabled).toBe(false);

    // Enable
    r = await request.put(settingsUrl, {
      data: { enabled: true },
      timeout: REQUEST_TIMEOUT,
    });
    expect(r.status()).toBe(200);
    expect((await r.json()).enabled).toBe(true);
    check = await request.get(settingsUrl, { timeout: REQUEST_TIMEOUT });
    expect((await check.json()).enabled).toBe(true);

    // Restore original value
    if (!original) {
      await request.put(settingsUrl, {
        data: { enabled: false },
        timeout: REQUEST_TIMEOUT,
      });
    }
  });
});

// ---------------------------------------------------------------------------
// POST /search
// ---------------------------------------------------------------------------

test.describe("TestSearchEndpoint", () => {
  test("search returns 200", async ({ request }) => {
    const response = await search(request, "test");

    expect(response.status()).toBe(200);
    const data = await response.json();
    expect(data).toHaveProperty("results");
    expect(Array.isArray(data.results)).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------
// Full pipeline: Queue URL → Crawl → Index → Search
// ---------------------------------------------------------------------------

test.describe("TestFullPipeline", () => {
  test("full crawl and search pipeline", async ({ request }) => {
    const testUrl = "https://en.wikipedia.org/wiki/Linux";
    const testDomain = "en.wikipedia.org";
    const searchTerm = "Linux";

    console.log(`\n1. Testing with URL: ${testUrl}`);

    // Arrange: add domain to allowed list
    console.log(`2. Adding domain '${testDomain}' to allowed list...`);
    const addResp = await addAllowedDomain(request, testDomain);
    expect(addResp.ok()).toBeTruthy();
    const addResult = await addResp.json();
    console.log(`   Done: ${addResult.message}`);

    // Act: add URL to crawl queue
    console.log("3. Adding URL to queue...");
    const queueResp = await addToQueue(request, testUrl);
    expect([200, 201]).toContain(queueResp.status());
    console.log("   Done: URL queued");

    // Assert: poll search until URL appears or timeout
    console.log(
      `4. Waiting for crawl and indexing (max ${TEST_TIMEOUT / 1000}s)...`,
    );
    let found = false;
    const startTime = Date.now();

    while (Date.now() - startTime < TEST_TIMEOUT) {
      await sleep(2000);
      try {
        const searchResp = await search(request, searchTerm);
        if (searchResp.status() === 200) {
          const results = await searchResp.json();
          if (results.results?.length) {
            const urls = results.results.map(
              (r: { document: { url?: string } }) => r.document?.url,
            );
            if (urls.includes(testUrl)) {
              const elapsed = ((Date.now() - startTime) / 1000).toFixed(1);
              console.log(
                `   Done: Page indexed and searchable (${elapsed}s)`,
              );
              found = true;
              break;
            }
            console.log(
              `   ... Found ${results.results.length} results, waiting for our URL...`,
            );
          } else {
            console.log("   ... No results yet, waiting...");
          }
        }
      } catch (e) {
        console.log(`   ... Search API error (retrying): ${e}`);
      }
    }

    expect(found).toBeTruthy();

    // Assert: verify search quality
    console.log("5. Verifying search quality...");
    const finalResp = await search(request, searchTerm);
    expect(finalResp.status()).toBe(200);
    const finalResults = await finalResp.json();
    expect(finalResults.results.length).toBeGreaterThanOrEqual(1);

    const topUrls = finalResults.results
      .slice(0, 3)
      .map((r: { document: { url: string } }) => r.document.url);
    expect(topUrls).toContain(testUrl);

    console.log(
      `   Done: Found ${finalResults.results.length} results, our URL in top 3`,
    );
    console.log("\nE2E test passed!");
  });
});
