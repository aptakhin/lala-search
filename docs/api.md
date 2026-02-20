# LalaSearch API Reference

Base URL: `http://localhost:3000`

## Authentication

LalaSearch uses passwordless magic link authentication. Users receive email links to sign in.

### Request magic link (send auth email)

Request a magic link to be sent to an email address. This sends an authentication email to the user.

```bash
curl -X POST http://localhost:3000/auth/request-link \
  -H "Content-Type: application/json" \
  -d '{"email": "user@example.com"}'
```

Response:
```json
{
  "success": true,
  "message": "If an account exists for this email, a magic link has been sent."
}
```

**Note**: The response is intentionally vague to prevent email enumeration. The email will only be sent if the user exists or if auto-registration is enabled.

**Email setup required**: Configure SMTP settings in `.env`:
```bash
SMTP_HOST=postfix          # or your SMTP server
SMTP_PORT=25               # or 587 for TLS
SMTP_USERNAME=             # empty for local Postfix
SMTP_PASSWORD=             # empty for local Postfix
SMTP_TLS=false             # true for external SMTP
SMTP_FROM_EMAIL=noreply@yourdomain.com
```

### Verify magic link

This endpoint is called automatically when the user clicks the link in their email. It creates a session and redirects to the app.

```bash
curl http://localhost:3000/auth/verify/TOKEN_FROM_EMAIL
```

On success: Redirects to `/` with session cookie set.

On failure:
```json
{
  "success": false,
  "message": "Verification failed: Token expired",
  "redirect_url": null
}
```

### Get current user info

Get information about the currently authenticated user.

```bash
curl http://localhost:3000/auth/me \
  -H "Cookie: lalasearch_session=YOUR_SESSION_TOKEN"
```

Response:
```json
{
  "user_id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "user@example.com",
  "email_verified": true,
  "organizations": [
    {
      "tenant_id": "default",
      "name": "",
      "role": "owner"
    }
  ]
}
```

### Sign out

Clear the current session.

```bash
curl -X POST http://localhost:3000/auth/signout \
  -H "Cookie: lalasearch_session=YOUR_SESSION_TOKEN"
```

Response:
```json
{
  "success": true,
  "message": "Signed out successfully"
}
```

### Invite user to organization (sends invitation email)

Invite a user to join an organization. This sends an invitation email.

**Requires**: Admin or Owner role in the organization.

```bash
curl -X POST http://localhost:3000/auth/organizations/default/invite \
  -H "Content-Type: application/json" \
  -H "Cookie: lalasearch_session=YOUR_SESSION_TOKEN" \
  -d '{
    "email": "newuser@example.com",
    "role": "member"
  }'
```

**Roles**: `owner`, `admin`, `member`

Response:
```json
{
  "success": true,
  "message": "Invitation sent to newuser@example.com"
}
```

### Accept invitation

This endpoint is called automatically when the user clicks the invitation link in their email.

```bash
curl http://localhost:3000/auth/invitations/INVITATION_TOKEN/accept
```

On success: Redirects to `/` with session cookie set.

### List organizations

Get all organizations the current user belongs to.

```bash
curl http://localhost:3000/auth/organizations \
  -H "Cookie: lalasearch_session=YOUR_SESSION_TOKEN"
```

Response:
```json
{
  "organizations": [
    {
      "tenant_id": "default",
      "name": "",
      "role": "owner"
    }
  ],
  "count": 1
}
```

### List organization members

Get all members of an organization.

```bash
curl http://localhost:3000/auth/organizations/default/members \
  -H "Cookie: lalasearch_session=YOUR_SESSION_TOKEN"
```

Response:
```json
{
  "members": [
    {
      "user_id": "550e8400-e29b-41d4-a716-446655440000",
      "email": "",
      "role": "owner",
      "joined_at": "2026-01-18T12:00:00Z"
    }
  ],
  "count": 1
}
```

### Remove organization member

Remove a member from an organization.

**Requires**: Admin or Owner role.

```bash
curl -X DELETE http://localhost:3000/auth/organizations/default/members/USER_ID \
  -H "Cookie: lalasearch_session=YOUR_SESSION_TOKEN"
```

Response:
```json
{
  "success": true,
  "message": "Member removed successfully"
}
```

## Allowed Domains

Manage the whitelist of domains permitted for crawling.

### List all allowed domains

```bash
curl http://localhost:3000/admin/allowed-domains
```

Response:
```json
{
  "domains": [
    {
      "domain": "example.com",
      "added_at": "2026-01-18T12:00:00Z",
      "added_by": "api",
      "notes": "Main site"
    }
  ],
  "count": 1
}
```

### Add a domain

```bash
curl -X POST http://localhost:3000/admin/allowed-domains \
  -H "Content-Type: application/json" \
  -d '{"domain": "example.com", "notes": "Optional description"}'
```

Response:
```json
{
  "success": true,
  "message": "Domain added to allowed list successfully",
  "domain": "example.com"
}
```

### Remove a domain

```bash
curl -X DELETE http://localhost:3000/admin/allowed-domains/example.com
```

Response:
```json
{
  "success": true,
  "message": "Domain removed from allowed list successfully",
  "domain": "example.com"
}
```

## Crawling Settings

Control crawler behavior at runtime without restarting the service.

### Get crawling status

```bash
curl http://localhost:3000/admin/settings/crawling-enabled
```

Response:
```json
{
  "enabled": true
}
```

### Disable crawling

Useful when testing API without crawler interference:

```bash
curl -X PUT http://localhost:3000/admin/settings/crawling-enabled \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}'
```

### Enable crawling

```bash
curl -X PUT http://localhost:3000/admin/settings/crawling-enabled \
  -H "Content-Type: application/json" \
  -d '{"enabled": true}'
```

## Queue Management

### Add URL to crawl queue

```bash
curl -X POST http://localhost:3000/queue/add \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com/page", "priority": 1}'
```

Note: The domain must be in the allowed domains list first.

## Search

### Search indexed documents

```bash
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": "search terms", "limit": 10}'
```

## Version

### Get agent version

```bash
curl http://localhost:3000/version
```

Response:
```json
{
  "agent": "lala-agent",
  "version": "0.1.0"
}
```
