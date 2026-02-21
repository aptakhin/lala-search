#!/usr/bin/env python3
"""
Shared fixtures and helpers for LalaSearch E2E tests.

Provides:
  - Mailtrap sandbox API helper to retrieve magic-link tokens from intercepted emails
  - Auth helpers for the full magic-link flow and invitation-acceptance flow

Environment variables required for multi-tenant tests:
  MAILTRAP_API_TOKEN  - Mailtrap API access token
  MAILTRAP_ACCOUNT_ID - Mailtrap account ID
  MAILTRAP_INBOX_ID   - Mailtrap inbox ID
  LALA_AGENT_URL      - Agent base URL (default: http://localhost:3000)
"""

import os
import re
import time

import httpx

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

AGENT_URL = os.environ.get("LALA_AGENT_URL", "http://localhost:3000")
MAILTRAP_API_TOKEN = os.environ.get("MAILTRAP_API_TOKEN", "")
MAILTRAP_ACCOUNT_ID = os.environ.get("MAILTRAP_ACCOUNT_ID", "")
MAILTRAP_INBOX_ID = os.environ.get("MAILTRAP_INBOX_ID", "")

REQUEST_TIMEOUT = 10   # seconds per HTTP request
EMAIL_WAIT_TIMEOUT = 60  # seconds to wait for email to arrive


# ---------------------------------------------------------------------------
# Mailtrap sandbox API helpers
# ---------------------------------------------------------------------------

def _require_mailtrap():
    """Raise RuntimeError if Mailtrap credentials are not configured."""
    if not (MAILTRAP_API_TOKEN and MAILTRAP_ACCOUNT_ID and MAILTRAP_INBOX_ID):
        raise RuntimeError(
            "MAILTRAP_API_TOKEN, MAILTRAP_ACCOUNT_ID, and MAILTRAP_INBOX_ID "
            "must be set to run multi-tenant auth tests."
        )


def _mailtrap_headers() -> dict:
    return {"Api-Token": MAILTRAP_API_TOKEN}


def _messages_url() -> str:
    return (
        f"https://sandbox.api.mailtrap.io/api/accounts/"
        f"{MAILTRAP_ACCOUNT_ID}/inboxes/{MAILTRAP_INBOX_ID}/messages"
    )


def clear_mailtrap_inbox():
    """Delete all messages in the configured Mailtrap inbox."""
    _require_mailtrap()
    clean_url = (
        f"https://sandbox.api.mailtrap.io/api/accounts/"
        f"{MAILTRAP_ACCOUNT_ID}/inboxes/{MAILTRAP_INBOX_ID}/clean"
    )
    resp = httpx.patch(clean_url, headers=_mailtrap_headers(), timeout=REQUEST_TIMEOUT)
    if resp.status_code not in (200, 204):
        print(f"[conftest] Warning: Mailtrap inbox clear returned {resp.status_code}")


def get_magic_link_token(to_email: str, timeout: int = EMAIL_WAIT_TIMEOUT) -> str:
    """
    Poll the Mailtrap inbox until an email addressed to *to_email* arrives.
    Extracts and returns the magic-link token (64-character hex string) from the body.
    Deletes the message after reading to keep the inbox clean.

    Raises TimeoutError if no matching email arrives within *timeout* seconds.
    """
    _require_mailtrap()
    headers = _mailtrap_headers()
    messages_url = _messages_url()
    deadline = time.time() + timeout

    while time.time() < deadline:
        resp = httpx.get(messages_url, headers=headers, timeout=REQUEST_TIMEOUT)
        if resp.status_code != 200:
            time.sleep(2)
            continue

        for msg in resp.json():
            to_field = (msg.get("to_email") or "").lower()
            if to_email.lower() not in to_field:
                continue

            msg_id = msg["id"]

            # Fetch the plain-text body (more reliable for token extraction)
            body_resp = httpx.get(
                f"{messages_url}/{msg_id}/body.txt",
                headers=headers,
                timeout=REQUEST_TIMEOUT,
            )
            body = body_resp.text if body_resp.status_code == 200 else ""

            # Fallback to HTML body
            if not body:
                body_resp = httpx.get(
                    f"{messages_url}/{msg_id}/body.html",
                    headers=headers,
                    timeout=REQUEST_TIMEOUT,
                )
                body = body_resp.text if body_resp.status_code == 200 else ""

            # Extract token from /auth/verify/{64-char hex token}
            match = re.search(r"/auth/verify/([a-f0-9]{64})", body)
            if match:
                token = match.group(1)
                # Delete message to keep inbox tidy
                httpx.delete(
                    f"{messages_url}/{msg_id}",
                    headers=headers,
                    timeout=REQUEST_TIMEOUT,
                )
                return token

        time.sleep(2)

    raise TimeoutError(
        f"Magic-link email to '{to_email}' not received within {timeout}s"
    )


# ---------------------------------------------------------------------------
# Auth helpers
# ---------------------------------------------------------------------------

def authenticate_via_magic_link(email: str, agent_url: str = AGENT_URL) -> str:
    """
    Full magic-link authentication flow:
      1. POST /auth/request-link  → agent sends email via SMTP → Mailtrap captures it
      2. Poll Mailtrap API until email with magic-link token arrives
      3. GET /auth/verify/{token} (no redirect follow) → agent sets lala_session cookie
      4. Return the session token value

    The returned token can be passed as the lala_session cookie in subsequent requests.
    """
    # Step 1: Request magic link
    resp = httpx.post(
        f"{agent_url}/auth/request-link",
        json={"email": email},
        timeout=REQUEST_TIMEOUT,
    )
    resp.raise_for_status()

    # Step 2: Retrieve token from Mailtrap
    token = get_magic_link_token(email)

    # Step 3: Verify token — agent returns 302 with Set-Cookie: lala_session=...
    verify_resp = httpx.get(
        f"{agent_url}/auth/verify/{token}",
        follow_redirects=False,
        timeout=REQUEST_TIMEOUT,
    )
    assert verify_resp.status_code == 302, (
        f"Expected 302 redirect after /auth/verify, got {verify_resp.status_code}: "
        f"{verify_resp.text}"
    )

    # Step 4: Extract session cookie
    session = verify_resp.cookies.get("lala_session")
    assert session, f"No lala_session cookie in verify response for {email}"
    return session


def accept_invitation(raw_token: str, agent_url: str = AGENT_URL) -> str:
    """
    Accept a pre-seeded organization invitation using its raw (unhashed) token.
    Returns the session cookie value scoped to the invitation's tenant.

    The invitation must already exist in the database (typically inserted by run_tests.sh).
    """
    resp = httpx.get(
        f"{agent_url}/auth/invitations/{raw_token}/accept",
        follow_redirects=False,
        timeout=REQUEST_TIMEOUT,
    )
    assert resp.status_code == 302, (
        f"Expected 302 after invitation accept, got {resp.status_code}: {resp.text}"
    )
    session = resp.cookies.get("lala_session")
    assert session, "No lala_session cookie in invitation accept response"
    return session
