#!/usr/bin/env python3
"""
Multi-Tenant End-to-End Tests for LalaSearch

Verifies that a single agent instance serving multiple tenants provides proper
data isolation: each tenant's data (domains, crawl queue, search index) is
visible only within that tenant's session.

Prerequisites (set up by run_tests.sh):
  - Agent running with DEPLOYMENT_MODE=multi_tenant
  - lalasearch_test keyspace (tenant 1 — default test tenant)
  - lalasearch_test_tenant2 keyspace (tenant 2)
  - lalasearch_system.tenants rows for both keyspaces
  - Pre-seeded org_invitation for user2@test.e2e → lalasearch_test_tenant2
    with raw token "e2e-test-tenant2-invite-0001"

Environment variables required:
  MAILTRAP_API_TOKEN   — Mailtrap API token
  MAILTRAP_ACCOUNT_ID  — Mailtrap account ID
  MAILTRAP_INBOX_ID    — Mailtrap inbox ID

Run via:
  ./tests/e2e/run_tests.sh
  # or manually (after setting up the environment):
  cd tests/e2e && DEPLOYMENT_MODE=multi_tenant uv run pytest test_multi_tenant.py -v
"""

import time

import httpx
import pytest

from conftest import (
    AGENT_URL,
    REQUEST_TIMEOUT,
    accept_invitation,
    authenticate_via_magic_link,
    clear_mailtrap_inbox,
)

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

# Raw invitation token pre-seeded by run_tests.sh for user2 → tenant2
TENANT2_INVITE_TOKEN = "e2e-test-tenant2-invite-0001"

# Emails used in tests — must match what run_tests.sh seeds
USER1_EMAIL = "user1@test.e2e"
USER2_EMAIL = "user2@test.e2e"

TEST_TIMEOUT = 90       # seconds to wait for crawl + indexing
POLL_INTERVAL = 3       # seconds between search polls


# ---------------------------------------------------------------------------
# Session helpers
# ---------------------------------------------------------------------------

def _cookies(session_token: str) -> dict:
    """Return cookie dict with lala_session set."""
    return {"lala_session": session_token}


def add_allowed_domain(domain: str, session: str, notes: str = "E2E multi-tenant test"):
    """Add a domain to the allowed list using the given session."""
    response = httpx.post(
        f"{AGENT_URL}/admin/allowed-domains",
        json={"domain": domain, "notes": notes},
        cookies=_cookies(session),
        timeout=REQUEST_TIMEOUT,
    )
    response.raise_for_status()
    return response.json()


def delete_allowed_domain(domain: str, session: str):
    """Remove a domain from the allowed list using the given session."""
    response = httpx.delete(
        f"{AGENT_URL}/admin/allowed-domains/{domain}",
        cookies=_cookies(session),
        timeout=REQUEST_TIMEOUT,
    )
    response.raise_for_status()
    return response.json()


def list_allowed_domains(session: str):
    """List all allowed domains using the given session."""
    response = httpx.get(
        f"{AGENT_URL}/admin/allowed-domains",
        cookies=_cookies(session),
        timeout=REQUEST_TIMEOUT,
    )
    response.raise_for_status()
    return response.json()


def add_to_queue(url: str, session: str, priority: int = 1):
    """Queue a URL for crawling using the given session."""
    response = httpx.post(
        f"{AGENT_URL}/queue/add",
        json={"url": url, "priority": priority},
        cookies=_cookies(session),
        timeout=REQUEST_TIMEOUT,
    )
    return response


def search(query: str, session: str, limit: int = 10):
    """Run a search query using the given session."""
    response = httpx.post(
        f"{AGENT_URL}/search",
        json={"query": query, "limit": limit},
        cookies=_cookies(session),
        timeout=REQUEST_TIMEOUT,
    )
    response.raise_for_status()
    return response.json()


def unique_domain(prefix: str = "mt") -> str:
    """Generate a unique test domain name."""
    return f"{prefix}-{int(time.time() * 1000)}.example.invalid"


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture(scope="module")
def sessions():
    """
    Authenticate both test users once per module and return their session tokens.

    Returns a dict with:
      sessions["user1"] — session scoped to lalasearch_test (default tenant)
      sessions["user2"] — session scoped to lalasearch_test_tenant2

    user1 authenticates via magic link (Mailtrap).
    user2 authenticates via the pre-seeded invitation.
    """
    clear_mailtrap_inbox()

    user1_session = authenticate_via_magic_link(USER1_EMAIL)
    user2_session = accept_invitation(TENANT2_INVITE_TOKEN)

    return {"user1": user1_session, "user2": user2_session}


# ---------------------------------------------------------------------------
# Connectivity smoke test
# ---------------------------------------------------------------------------

class TestAgentConnectivity:
    """Agent must be reachable and in multi-tenant mode."""

    def test_agent_is_healthy(self):
        """GET /version returns 200 with valid fields."""
        response = httpx.get(f"{AGENT_URL}/version", timeout=REQUEST_TIMEOUT)
        assert response.status_code == 200
        data = response.json()
        assert "version" in data
        assert data["deployment_mode"] == "multi_tenant", (
            f"Expected deployment_mode=multi_tenant, got: {data['deployment_mode']}"
        )


# ---------------------------------------------------------------------------
# Tenant isolation — allowed domains
# ---------------------------------------------------------------------------

class TestTenantDomainIsolation:
    """Domains added within one tenant session must not appear in another tenant."""

    def test_domain_added_by_user2_not_visible_to_user1(self, sessions):
        """
        Arrange: unique domain.
        Act: add via user2 session (tenant2).
        Assert: visible in tenant2 list; absent from tenant1 list.
        """
        domain = unique_domain("iso")

        add_allowed_domain(domain, session=sessions["user2"])

        tenant2_domains = [d["domain"] for d in list_allowed_domains(sessions["user2"])["domains"]]
        tenant1_domains = [d["domain"] for d in list_allowed_domains(sessions["user1"])["domains"]]

        assert domain in tenant2_domains, "domain must be visible to tenant2"
        assert domain not in tenant1_domains, "domain must NOT be visible to tenant1"

        # Cleanup
        delete_allowed_domain(domain, session=sessions["user2"])

    def test_domain_added_by_user1_not_visible_to_user2(self, sessions):
        """
        Arrange: unique domain.
        Act: add via user1 session (default tenant).
        Assert: visible in tenant1 list; absent from tenant2 list.
        """
        domain = unique_domain("iso-rev")

        add_allowed_domain(domain, session=sessions["user1"])

        tenant1_domains = [d["domain"] for d in list_allowed_domains(sessions["user1"])["domains"]]
        tenant2_domains = [d["domain"] for d in list_allowed_domains(sessions["user2"])["domains"]]

        assert domain in tenant1_domains, "domain must be visible to tenant1"
        assert domain not in tenant2_domains, "domain must NOT be visible to tenant2"

        # Cleanup
        delete_allowed_domain(domain, session=sessions["user1"])


# ---------------------------------------------------------------------------
# Full multi-tenant crawl workflow
# ---------------------------------------------------------------------------

class TestTenant2CrawlWorkflow:
    """
    End-to-end: tenant2 adds an allowed domain, queues a URL, and the scheduler
    automatically crawls and indexes it — without polluting tenant1's search index.
    """

    def test_add_domain_queue_url_and_scheduler_crawls_it(self, sessions):
        """
        Arrange: clean tenant2 state.
        Act: add domain → queue Wikipedia URL via tenant2 session.
        Assert:
          - Tenant2 search returns the indexed URL within TEST_TIMEOUT seconds.
          - Tenant1 search does NOT contain the same URL (isolation).
        """
        test_url = "https://en.wikipedia.org/wiki/Linux"
        test_domain = "en.wikipedia.org"
        search_term = "Linux"

        print(f"\n[multi-tenant] Testing with URL: {test_url}")

        # --- Arrange: add domain to tenant2 allow list ---
        print(f"[multi-tenant] Adding '{test_domain}' to tenant2 allow list...")
        add_result = add_allowed_domain(test_domain, session=sessions["user2"])
        assert add_result["success"] is True
        print(f"   ✓ {add_result['message']}")

        # --- Act: queue the URL via tenant2 ---
        print("[multi-tenant] Queuing URL via tenant2...")
        queue_resp = add_to_queue(test_url, session=sessions["user2"])
        assert queue_resp.status_code in (200, 201), (
            f"Failed to queue URL: {queue_resp.status_code} — {queue_resp.text}"
        )
        print("   ✓ URL queued in tenant2")

        # --- Assert: poll tenant2 search until URL is indexed or timeout ---
        print(f"[multi-tenant] Waiting for scheduler to crawl and index (max {TEST_TIMEOUT}s)...")
        found_in_tenant2 = False
        start = time.time()

        while time.time() - start < TEST_TIMEOUT:
            time.sleep(POLL_INTERVAL)
            try:
                results = search(search_term, session=sessions["user2"])
                urls = [r["document"].get("url") for r in results.get("results", [])]
                if test_url in urls:
                    elapsed = time.time() - start
                    print(f"   ✓ URL indexed in tenant2 ({elapsed:.1f}s)")
                    found_in_tenant2 = True
                    break
                count = len(results.get("results", []))
                print(f"   ... {count} result(s) in tenant2, waiting for our URL...")
            except httpx.HTTPError as exc:
                print(f"   ... tenant2 search error (retrying): {exc}")

        assert found_in_tenant2, (
            f"URL '{test_url}' not found in tenant2 search results after {TEST_TIMEOUT}s"
        )

        # --- Assert: verify tenant1 does NOT have this URL ---
        print("[multi-tenant] Verifying tenant1 does NOT have the URL (isolation check)...")
        try:
            tenant1_results = search(search_term, session=sessions["user1"])
            tenant1_urls = [
                r["document"].get("url") for r in tenant1_results.get("results", [])
            ]
            assert test_url not in tenant1_urls, (
                f"URL '{test_url}' must NOT appear in tenant1 search results"
            )
            print("   ✓ URL correctly absent from tenant1 search")
        except httpx.HTTPStatusError as exc:
            # 503 means search is unavailable for tenant1 — acceptable (no results)
            if exc.response.status_code != 503:
                raise

        print("\n✅ Multi-tenant E2E test passed!")

        # --- Cleanup ---
        delete_allowed_domain(test_domain, session=sessions["user2"])
