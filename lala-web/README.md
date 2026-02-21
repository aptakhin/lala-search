# LalaSearch Web Frontend

A retro 1990s-style web interface for LalaSearch built with plain HTML, CSS, and Alpine.js.

## Features

- **Retro Design**: Authentic 1990s aesthetic with beveled buttons and classic colors
- **Alpine.js**: Lightweight interactivity without heavy build tools or npm dependencies
- **Search Interface**: Clean search box with real-time result display
- **Magic Link Auth**: Passwordless sign-in via email
- **Dashboard**: Invite users and manage allowed domains
- **Pagination**: Navigate through search results with previous/next buttons
- **Responsive**: Works on desktop and mobile browsers
- **Nginx Served**: Fast static file serving with API proxy

## Architecture

```
lala-web/
├── html/                       # Served by Nginx
│   ├── index.html              # Search page (/)
│   ├── styles.css              # Shared CSS for all pages
│   ├── signin/
│   │   └── index.html          # Sign-in page (/signin)
│   └── dashboard/
│       └── index.html          # Dashboard page (/dashboard)
├── nginx.conf                  # Nginx configuration for routing and API proxy
├── Dockerfile                  # Container image for the web service
└── README.md                   # This file
```

## Pages

### Search (`/`)

Public search interface. In **multi-tenant** mode, shows a "Sign in for Own Search" button in the top-right corner. In **single-tenant** mode, automatically redirects to sign-in.

### Sign In (`/signin`)

Email input with "Send Sign-In Email" button. On submit, the backend sends a magic link email. Clicking the magic link verifies the user and redirects to the dashboard.

If already authenticated, redirects to `/dashboard`.

### Dashboard (`/dashboard`)

Private page requiring authentication. Shows:

- **User info bar**: Email, role, sign out button
- **Invite Users** (owners/admins): Email input, role selector, current members table
- **Allowed Domains** (owners/admins): Domain input, current domains table

Unauthenticated users are redirected to `/signin`.

## Nginx Routing

| Path | Target | Purpose |
|------|--------|---------|
| `/` | `html/index.html` | Search page |
| `/signin` | `html/signin/index.html` | Sign-in page |
| `/dashboard` | `html/dashboard/index.html` | Dashboard page |
| `/api/*` | `lala-agent:3000/*` | Backend API proxy |
| `/auth/verify/*` | `lala-agent:3000/auth/verify/*` | Magic link verification |
| `/auth/invitations/*` | `lala-agent:3000/auth/invitations/*` | Invitation acceptance |
| `/health` | `200 OK` | Health check |

## Running

Start all services including the web frontend:

```bash
docker compose up -d --build
```

Access the web UI at: **http://localhost:8081**

## Technologies

- **HTML5**: Semantic markup
- **CSS3**: Retro styling with flexbox
- **Alpine.js 3.x**: Lightweight interactivity (loaded from CDN)
- **Nginx**: Reverse proxy and static serving
- **Docker**: Container deployment

## No External Dependencies

This frontend has **zero npm dependencies**:
- No webpack, parcel, vite builds
- No node_modules
- No transpilation needed
- Alpine.js loaded directly from CDN

## Playwright Test IDs

All interactive elements have `data-testid` attributes for E2E testing:

**Search page**: `search-input`, `search-button`, `search-clear-button`, `search-results`, `search-error-message`, `signin-nav-button`, `dashboard-nav-button`

**Sign-in page**: `signin-email-input`, `signin-submit-button`, `signin-success-message`, `signin-error-message`

**Dashboard**: `dashboard-user-email`, `dashboard-user-role`, `signout-button`, `invite-email-input`, `invite-role-select`, `invite-submit-button`, `invite-success-message`, `invite-error-message`, `members-table`, `remove-member-button`, `domain-input`, `domain-notes-input`, `add-domain-button`, `domain-success-message`, `domain-error-message`, `domains-table`, `delete-domain-button`

## Development

To modify the interface:

1. Edit files in `lala-web/html/`
2. Docker volume mount provides live updates (no rebuild needed)
3. Or rebuild container: `docker compose up -d --build lala-web`

## Browser Support

Works in all modern browsers:
- Chrome/Edge 60+
- Firefox 55+
- Safari 11+
- Mobile browsers (iOS Safari, Chrome Android)

## License

SPDX-License-Identifier: BSD-3-Clause
Copyright (c) 2026 Aleksandr Ptakhin
