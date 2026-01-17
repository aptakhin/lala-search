#!/usr/bin/env python3
"""
End-to-End System Test for LalaSearch

Tests the full pipeline: Queue URL → Crawl → Index → Search
Uses only public APIs, no internal state inspection.
"""

import time
import requests
import pytest

# Configuration
AGENT_URL = "http://localhost:3000"
TEST_TIMEOUT = 60  # seconds


def test_full_crawl_and_search_pipeline():
    """
    E2E test: Add URL to queue → Wait for crawl → Search → Verify results

    This test verifies:
    - API accepts queue additions
    - Crawler processes the URL
    - Content is indexed
    - Search returns results
    """

    # Use a simple, stable Wikipedia page with known content
    # "Linux" article is stable and has predictable content
    test_url = "https://en.wikipedia.org/wiki/Linux"
    test_domain = "en.wikipedia.org"
    search_term = "Linux"  # Should definitely be in the article

    print(f"\n1. Testing with URL: {test_url}")

    # Step 1: Add domain to allowed list
    print(f"2. Adding domain '{test_domain}' to allowed list...")
    response = requests.post(
        f"{AGENT_URL}/admin/allowed-domains",
        json={"domain": test_domain}
    )
    assert response.status_code in [200, 201], \
        f"Failed to add domain: {response.status_code} - {response.text}"
    print("   ✓ Domain added")

    # Step 2: Add URL to crawl queue
    print(f"3. Adding URL to queue...")
    response = requests.post(
        f"{AGENT_URL}/queue/add",
        json={"url": test_url, "priority": 1}
    )
    assert response.status_code in [200, 201], \
        f"Failed to add to queue: {response.status_code} - {response.text}"
    print("   ✓ URL queued")

    # Step 3: Wait for crawling and indexing (poll search API)
    print(f"4. Waiting for crawl and indexing (max {TEST_TIMEOUT}s)...")
    found = False
    start_time = time.time()

    while time.time() - start_time < TEST_TIMEOUT:
        time.sleep(2)  # Poll every 2 seconds

        # Search for content we know is on the page
        try:
            response = requests.get(
                f"{AGENT_URL}/search",
                params={"q": search_term, "limit": 10}
            )

            if response.status_code == 200:
                results = response.json()

                # Check if we got results and our URL is in them
                if results.get("hits") and len(results["hits"]) > 0:
                    urls = [hit.get("url") for hit in results["hits"]]
                    if test_url in urls:
                        elapsed = time.time() - start_time
                        print(f"   ✓ Page indexed and searchable ({elapsed:.1f}s)")
                        found = True
                        break
                    else:
                        print(f"   ... Found {len(results['hits'])} results, waiting for our URL...")
                else:
                    print(f"   ... No results yet, waiting...")
        except requests.exceptions.RequestException as e:
            print(f"   ... Search API error (retrying): {e}")
            continue

    assert found, \
        f"URL '{test_url}' not found in search results after {TEST_TIMEOUT}s"

    # Step 4: Verify search quality (optional but good to check)
    print(f"5. Verifying search quality...")
    response = requests.get(
        f"{AGENT_URL}/search",
        params={"q": search_term, "limit": 10}
    )

    assert response.status_code == 200, "Search API should be accessible"
    results = response.json()

    # Should have at least our page
    assert len(results["hits"]) >= 1, "Should return at least one result"

    # Our URL should be in the top results
    top_urls = [hit["url"] for hit in results["hits"][:3]]
    assert test_url in top_urls, "Our URL should be in top 3 results"

    print(f"   ✓ Found {len(results['hits'])} results, our URL in top 3")
    print("\n✅ E2E test passed!")


def test_search_api_available():
    """Smoke test: Verify search API is accessible"""
    response = requests.get(f"{AGENT_URL}/search", params={"q": "test"})
    assert response.status_code == 200, "Search API should be accessible"


def test_version_endpoint():
    """Smoke test: Verify version endpoint works"""
    response = requests.get(f"{AGENT_URL}/version")
    assert response.status_code == 200, "Version endpoint should work"
    data = response.json()
    assert "version" in data, "Version should be in response"
    print(f"\nAgent version: {data['version']}")


if __name__ == "__main__":
    # Run the test directly
    print("="*60)
    print("LalaSearch E2E System Test")
    print("="*60)

    try:
        test_version_endpoint()
        test_search_api_available()
        test_full_crawl_and_search_pipeline()
    except AssertionError as e:
        print(f"\n❌ Test failed: {e}")
        exit(1)
    except Exception as e:
        print(f"\n❌ Unexpected error: {e}")
        import traceback
        traceback.print_exc()
        exit(1)
