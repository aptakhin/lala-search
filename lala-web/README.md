# LalaSearch Web Frontend

A retro 1990s-style web interface for LalaSearch built with HTML, CSS, Alpine.js, and TypeScript.

## Features

- **Retro Design**: Authentic 1990s aesthetic with beveled buttons and classic colors
- **Alpine.js**: Lightweight interactivity without heavy frameworks
- **TypeScript**: Type-safe JavaScript with esbuild bundling
- **Nginx SSI**: Server Side Includes for shared HTML partials (header, footer)
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
│   ├── includes/               # SSI partials (not directly accessible)
│   │   ├── head.html           # Shared meta tags + CSS link
│   │   ├── header.html         # Logo, tagline, auth button
│   │   └── footer.html         # Copyright footer
│   ├── js/                     # Built by esbuild (gitignored)
│   ├── signin/
│   │   └── index.html          # Sign-in page (/signin)
│   └── dashboard/
│       └── index.html          # Dashboard page (/dashboard)
├── src/                        # TypeScript source
│   ├── types/alpine.d.ts       # Minimal Alpine.js type declarations
│   ├── lib/api.ts              # Shared API fetch helpers
│   └── pages/
│       ├── search.ts           # Search page Alpine components
│       ├── signin.ts           # Sign-in page Alpine component
│       └── dashboard.ts        # Dashboard page Alpine components
├── package.json                # esbuild + typescript dev dependencies
├── tsconfig.json               # TypeScript config (type checking only)
├── nginx.conf                  # Nginx config with SSI enabled
├── Dockerfile                  # Multi-stage: node build + nginx serve
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

## Development

### Prerequisites

- Node.js 22+ (for TypeScript compilation)
- Docker (for running the full stack)

### Setup

```bash
cd lala-web
npm install
```

### Development workflow

1. Start the stack:
   ```bash
   docker compose up -d --build lala-web
   ```

2. Start the TypeScript watcher (in a separate terminal):
   ```bash
   cd lala-web
   npm run watch
   ```

3. Edit files and refresh your browser:
   - **TypeScript** (`src/**/*.ts`): esbuild rebuilds to `html/js/` in ~6ms
   - **HTML/CSS** (`html/**`): changes are instant (volume-mounted)
   - **SSI partials** (`html/includes/*`): changes are instant (nginx processes per-request)

### Available scripts

| Command | Description |
|---------|-------------|
| `npm run build` | Production build (minified, no sourcemaps) |
| `npm run watch` | Development build with file watching and sourcemaps |
| `npm run typecheck` | Run TypeScript type checking (no output) |

### How it works

- **esbuild** compiles TypeScript and bundles per-page JS files (`search.js`, `signin.js`, `dashboard.js`)
- **Nginx SSI** processes `<!--#include -->` directives at request time, assembling shared HTML partials
- Alpine.js is loaded from CDN. Page bundles use `defer` and appear before Alpine in document order, ensuring component functions are defined before Alpine initializes
- The `html/js/` directory is gitignored (build artifact) and volume-mounted into Docker for live updates

## Technologies

- **HTML5**: Semantic markup
- **CSS3**: Retro styling with flexbox
- **TypeScript**: Type-safe JavaScript (compiled by esbuild)
- **Alpine.js 3.x**: Lightweight interactivity (loaded from CDN)
- **esbuild**: Fast TypeScript bundler (~6ms rebuilds)
- **Nginx**: Reverse proxy, static serving, SSI
- **Docker**: Multi-stage container deployment

## Playwright Test IDs

All interactive elements have `data-testid` attributes for E2E testing:

**Search page**: `search-input`, `search-button`, `search-clear-button`, `search-results`, `search-error-message`, `signin-nav-button`, `dashboard-nav-button`

**Sign-in page**: `signin-email-input`, `signin-submit-button`, `signin-success-message`, `signin-error-message`

**Dashboard**: `dashboard-user-email`, `dashboard-user-role`, `signout-button`, `invite-email-input`, `invite-role-select`, `invite-submit-button`, `invite-success-message`, `invite-error-message`, `members-table`, `remove-member-button`, `domain-input`, `domain-notes-input`, `add-domain-button`, `domain-success-message`, `domain-error-message`, `domains-table`, `delete-domain-button`

## Browser Support

Works in all modern browsers:
- Chrome/Edge 60+
- Firefox 55+
- Safari 11+
- Mobile browsers (iOS Safari, Chrome Android)

## License

SPDX-License-Identifier: BSD-3-Clause
Copyright (c) 2026 Aleksandr Ptakhin
