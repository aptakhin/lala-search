#!/usr/bin/env python3
"""
End-to-End System Tests for LalaSearch

Tests the full HTTP API surface against a live agent.
Uses only public APIs, no internal state inspection.

Test classes group related endpoints (AAA structure within each test):
  - TestVersion          - GET /version
  - TestAdminDomains     - POST / GET / DELETE /admin/allowed-domains
  - TestQueueEndpoint    - POST /queue/add
  - TestCrawlingSettings - GET / PUT /admin/settings/crawling-enabled
  - TestSearchEndpoint   - POST /search
  - TestFullPipeline     - End-to-end: queue → crawl → index → search
"""

import time
import httpx
import pytest

# Configuration
AGENT_URL = "http://localhost:3000"
TEST_TIMEOUT = 60  # seconds to wait for crawl + indexing
REQUEST_TIMEOUT = 10  # seconds per individual HTTP request


# ---------------------------------------------------------------------------
# Shared helpers
# ---------------------------------------------------------------------------

def add_allowed_domain(domain: str, notes: str = "E2E test domain", agent_url: str = AGENT_URL):
    """Add a domain to the allowed list via HTTP API."""
    response = httpx.post(
        f"{agent_url}/admin/allowed-domains",
        json={"domain": domain, "notes": notes},
        timeout=REQUEST_TIMEOUT,
    )
    response.raise_for_status()
    return response.json()


def delete_allowed_domain(domain: str, agent_url: str = AGENT_URL):
    """Remove a domain from the allowed list via HTTP API."""
    response = httpx.delete(
        f"{agent_url}/admin/allowed-domains/{domain}",
        timeout=REQUEST_TIMEOUT,
    )
    response.raise_for_status()
    return response.json()


def list_allowed_domains(agent_url: str = AGENT_URL):
    """List all allowed domains via HTTP API."""
    response = httpx.get(
        f"{agent_url}/admin/allowed-domains",
        timeout=REQUEST_TIMEOUT,
    )
    response.raise_for_status()
    return response.json()


def unique_domain(prefix: str = "e2e") -> str:
    """Generate a unique test domain name that won't collide with real traffic."""
    return f"{prefix}-{int(time.time() * 1000)}.example.invalid"


# ---------------------------------------------------------------------------
# GET /version
# ---------------------------------------------------------------------------

class TestVersion:
    """Tests for GET /version endpoint."""

    def test_returns_200_with_version_fields(self):
        """Arrange: nothing. Act: GET /version. Assert: 200 + required fields."""
        response = httpx.get(f"{AGENT_URL}/version", timeout=REQUEST_TIMEOUT)

        assert response.status_code == 200
        data = response.json()
        assert "version" in data
        assert "agent" in data
        assert "deployment_mode" in data
        assert data["agent"] == "lala-agent"

    def test_version_follows_semver(self):
        """Assert: version string is MAJOR.MINOR.PATCH."""
        response = httpx.get(f"{AGENT_URL}/version", timeout=REQUEST_TIMEOUT)

        data = response.json()
        parts = data["version"].split(".")
        assert len(parts) == 3, f"expected semver, got: {data['version']}"
        assert all(p.isdigit() for p in parts), "each semver part must be numeric"

    def test_deployment_mode_is_valid(self):
        """Assert: deployment_mode is one of the known enum values."""
        response = httpx.get(f"{AGENT_URL}/version", timeout=REQUEST_TIMEOUT)

        data = response.json()
        assert data["deployment_mode"] in ("single_tenant", "multi_tenant"), (
            f"unexpected deployment_mode: {data['deployment_mode']}"
        )

    def test_unknown_route_returns_404(self):
        """Assert: a non-existent route returns 404."""
        response = httpx.get(f"{AGENT_URL}/does-not-exist", timeout=REQUEST_TIMEOUT)

        assert response.status_code == 404


# ---------------------------------------------------------------------------
# POST / GET / DELETE /admin/allowed-domains
# ---------------------------------------------------------------------------

class TestAdminDomains:
    """Tests for the allowed-domains admin endpoints."""

    def test_add_domain_success(self):
        """Arrange: unique domain. Act: POST add. Assert: success response."""
        domain = unique_domain("add")

        result = add_allowed_domain(domain, notes="add test")

        assert result["success"] is True
        assert result["domain"] == domain
        assert "Domain added" in result["message"]

        # Cleanup
        delete_allowed_domain(domain)

    def test_list_domains_returns_array(self):
        """Act: GET list. Assert: response has domains list and matching count."""
        result = list_allowed_domains()

        assert "domains" in result
        assert "count" in result
        assert result["count"] == len(result["domains"])

    def test_add_then_list_shows_domain(self):
        """Arrange: add domain. Act: list. Assert: domain present with correct metadata."""
        domain = unique_domain("list")
        add_allowed_domain(domain, notes="list test")

        result = list_allowed_domains()
        found = next((d for d in result["domains"] if d["domain"] == domain), None)

        assert found is not None, f"{domain} should appear in list after add"
        assert found["notes"] == "list test"
        assert found["added_by"] is not None

        # Cleanup
        delete_allowed_domain(domain)

    def test_delete_domain_removes_it_from_list(self):
        """Arrange: add domain. Act: delete + list. Assert: domain absent."""
        domain = unique_domain("del")
        add_allowed_domain(domain)

        delete_allowed_domain(domain)
        result = list_allowed_domains()
        domains_after = [d["domain"] for d in result["domains"]]

        assert domain not in domains_after

    def test_delete_nonexistent_domain_is_idempotent(self):
        """Act: delete domain that does not exist. Assert: 200 (idempotent)."""
        domain = unique_domain("ghost")

        result = delete_allowed_domain(domain)

        assert result["success"] is True

    def test_add_empty_domain_returns_400(self):
        """Act: POST with empty domain. Assert: 400 with descriptive error."""
        response = httpx.post(
            f"{AGENT_URL}/admin/allowed-domains",
            json={"domain": ""},
            timeout=REQUEST_TIMEOUT,
        )

        assert response.status_code == 400
        assert "Domain cannot be empty" in response.text


# ---------------------------------------------------------------------------
# POST /queue/add
# ---------------------------------------------------------------------------

class TestQueueEndpoint:
    """Tests for POST /queue/add."""

    def test_add_approved_domain_url_succeeds(self):
        """Arrange: add domain to allowlist. Act: queue URL. Assert: 200 success."""
        domain = unique_domain("queue")
        test_url = f"https://{domain}/page"
        add_allowed_domain(domain)

        response = httpx.post(
            f"{AGENT_URL}/queue/add",
            json={"url": test_url, "priority": 1},
            timeout=REQUEST_TIMEOUT,
        )

        assert response.status_code == 200
        data = response.json()
        assert data["success"] is True
        assert data["url"] == test_url
        assert data["domain"] == domain

        # Cleanup
        delete_allowed_domain(domain)

    def test_invalid_url_returns_400(self):
        """Act: queue a malformed URL. Assert: 400."""
        response = httpx.post(
            f"{AGENT_URL}/queue/add",
            json={"url": "not-a-valid-url", "priority": 1},
            timeout=REQUEST_TIMEOUT,
        )

        assert response.status_code == 400

    def test_unapproved_domain_returns_403(self):
        """Act: queue URL for a domain not in the allowlist. Assert: 403."""
        unapproved = unique_domain("forbidden")
        response = httpx.post(
            f"{AGENT_URL}/queue/add",
            json={"url": f"https://{unapproved}/page", "priority": 1},
            timeout=REQUEST_TIMEOUT,
        )

        assert response.status_code == 403
        assert "not in the allowed domains list" in response.text


# ---------------------------------------------------------------------------
# GET / PUT /admin/settings/crawling-enabled
# ---------------------------------------------------------------------------

class TestCrawlingSettings:
    """Tests for crawling-enabled settings endpoints."""

    def test_get_returns_boolean(self):
        """Act: GET crawling-enabled. Assert: 200 with boolean enabled field."""
        response = httpx.get(
            f"{AGENT_URL}/admin/settings/crawling-enabled",
            timeout=REQUEST_TIMEOUT,
        )

        assert response.status_code == 200
        data = response.json()
        assert "enabled" in data
        assert isinstance(data["enabled"], bool)

    def test_disable_then_enable_persists(self):
        """Arrange: read original. Act: disable → read → enable → read. Assert: each persists."""
        settings_url = f"{AGENT_URL}/admin/settings/crawling-enabled"
        original = httpx.get(settings_url, timeout=REQUEST_TIMEOUT).json()["enabled"]

        # Disable
        r = httpx.put(settings_url, json={"enabled": False}, timeout=REQUEST_TIMEOUT)
        assert r.status_code == 200
        assert r.json()["enabled"] is False
        assert httpx.get(settings_url, timeout=REQUEST_TIMEOUT).json()["enabled"] is False

        # Enable
        r = httpx.put(settings_url, json={"enabled": True}, timeout=REQUEST_TIMEOUT)
        assert r.status_code == 200
        assert r.json()["enabled"] is True
        assert httpx.get(settings_url, timeout=REQUEST_TIMEOUT).json()["enabled"] is True

        # Restore
        if not original:
            httpx.put(settings_url, json={"enabled": False}, timeout=REQUEST_TIMEOUT)


# ---------------------------------------------------------------------------
# POST /search
# ---------------------------------------------------------------------------

class TestSearchEndpoint:
    """Tests for POST /search."""

    def test_search_returns_200(self):
        """Act: search for any term. Assert: 200 with results array."""
        response = httpx.post(
            f"{AGENT_URL}/search",
            json={"query": "test"},
            timeout=REQUEST_TIMEOUT,
        )

        assert response.status_code == 200
        data = response.json()
        assert "results" in data
        assert isinstance(data["results"], list)


# ---------------------------------------------------------------------------
# Full pipeline: Queue URL → Crawl → Index → Search
# ---------------------------------------------------------------------------

class TestFullPipeline:
    """End-to-end pipeline test: queue URL → crawl → index → search."""

    def test_full_crawl_and_search_pipeline(self):
        """
        E2E test: Add URL to queue → Wait for crawl → Search → Verify results.

        Verifies:
        - API accepts queue additions
        - Crawler processes the URL
        - Content is indexed in Meilisearch
        - Search returns the crawled URL
        """
        test_url = "https://en.wikipedia.org/wiki/Linux"
        test_domain = "en.wikipedia.org"
        search_term = "Linux"

        print(f"\n1. Testing with URL: {test_url}")

        # Arrange: add domain to allowed list
        print(f"2. Adding domain '{test_domain}' to allowed list...")
        try:
            result = add_allowed_domain(test_domain)
            print(f"   ✓ Domain added: {result['message']}")
        except Exception as e:
            pytest.fail(f"Failed to add domain: {e}")

        # Act: add URL to crawl queue
        print("3. Adding URL to queue...")
        response = httpx.post(
            f"{AGENT_URL}/queue/add",
            json={"url": test_url, "priority": 1},
            timeout=REQUEST_TIMEOUT,
        )
        assert response.status_code in [200, 201], (
            f"Failed to add to queue: {response.status_code} - {response.text}"
        )
        print("   ✓ URL queued")

        # Assert: poll search until URL appears or timeout
        print(f"4. Waiting for crawl and indexing (max {TEST_TIMEOUT}s)...")
        found = False
        start_time = time.time()

        while time.time() - start_time < TEST_TIMEOUT:
            time.sleep(2)
            try:
                response = httpx.post(
                    f"{AGENT_URL}/search",
                    json={"query": search_term, "limit": 10},
                    timeout=REQUEST_TIMEOUT,
                )
                if response.status_code == 200:
                    results = response.json()
                    if results.get("results"):
                        urls = [r["document"].get("url") for r in results["results"]]
                        if test_url in urls:
                            elapsed = time.time() - start_time
                            print(f"   ✓ Page indexed and searchable ({elapsed:.1f}s)")
                            found = True
                            break
                        print(f"   ... Found {len(results['results'])} results, waiting for our URL...")
                    else:
                        print("   ... No results yet, waiting...")
            except httpx.HTTPError as e:
                print(f"   ... Search API error (retrying): {e}")

        assert found, f"URL '{test_url}' not found in search results after {TEST_TIMEOUT}s"

        # Assert: verify search quality
        print("5. Verifying search quality...")
        response = httpx.post(
            f"{AGENT_URL}/search",
            json={"query": search_term, "limit": 10},
            timeout=REQUEST_TIMEOUT,
        )
        assert response.status_code == 200
        results = response.json()
        assert len(results["results"]) >= 1

        top_urls = [r["document"]["url"] for r in results["results"][:3]]
        assert test_url in top_urls, "Our URL should be in top 3 results"

        print(f"   ✓ Found {len(results['results'])} results, our URL in top 3")
        print("\n✅ E2E test passed!")


if __name__ == "__main__":
    print("=" * 60)
    print("LalaSearch E2E System Test")
    print("=" * 60)

    try:
        t = TestVersion()
        t.test_returns_200_with_version_fields()
        t.test_version_follows_semver()

        s = TestSearchEndpoint()
        s.test_search_returns_200()

        p = TestFullPipeline()
        p.test_full_crawl_and_search_pipeline()
    except AssertionError as e:
        print(f"\n❌ Test failed: {e}")
        exit(1)
    except Exception as e:
        print(f"\n❌ Unexpected error: {e}")
        import traceback
        traceback.print_exc()
        exit(1)
