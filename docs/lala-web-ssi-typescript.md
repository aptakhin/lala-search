# Plan: Nginx SSI Templates + TypeScript with esbuild

## Context

lala-web currently has 3 HTML pages with duplicated headers, footers, meta tags, and Alpine.js CDN links copy-pasted across all files. All JavaScript (~400 lines) is inline in `<script>` blocks with no type safety. This plan adds two things with minimal infrastructure:

1. **Nginx SSI** (Server Side Includes) to eliminate HTML duplication — zero build tooling, built into nginx
2. **esbuild + TypeScript** to convert inline JS to typed external modules — just 2 npm dev dependencies

## Commit 1: Nginx SSI Templates

### Files to create

**`lala-web/html/includes/head.html`** — shared `<head>` content:
```html
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="stylesheet" href="/styles.css">
```

**`lala-web/html/includes/header.html`** — shared header (including auth button with `x-show` guards that evaluate to falsy on pages that don't define those variables):
```html
<header>
    <h1><a href="/">LALASEARCH</a></h1>
    <p class="tagline">The Open Source Search Engine - Retro Edition</p>
    <div class="auth-header-btn" x-show="ready && deploymentMode === 'multi_tenant'" x-cloak>
        <a x-show="!user" href="/signin" class="btn-signin" data-testid="signin-nav-button">Sign in for Own Search</a>
        <a x-show="user" href="/dashboard" class="btn-dashboard" data-testid="dashboard-nav-button">Dashboard</a>
    </div>
</header>
```

**`lala-web/html/includes/footer.html`** — shared footer:
```html
<footer>
    <p>LalaSearch &copy; 2026 | Open Source Distributed Search Engine</p>
    <p style="margin-top: 10px; font-size: 11px; color: #999999;">Designed in retro 1990s style</p>
</footer>
```

### Files to modify

**`lala-web/nginx.conf`** — add `ssi on;` and block direct access to includes:
```nginx
location / {
    root /usr/share/nginx/html;
    ssi on;
    try_files $uri $uri/ /index.html;
    add_header Cache-Control "no-cache, no-store, must-revalidate";
}

location /includes/ {
    internal;
}
```

**All 3 HTML pages** — replace duplicated blocks with SSI directives:
```html
<head>
    <!--#include virtual="/includes/head.html" -->
    <title>Page Title</title>
    <script defer src="https://cdn.jsdelivr.net/npm/alpinejs@3.x.x/dist/cdn.min.js"></script>
</head>
<body ...>
    <!--#include virtual="/includes/header.html" -->
    <!-- page content unchanged -->
    <!--#include virtual="/includes/footer.html" -->
    <script>/* inline JS stays for now */</script>
</body>
```

### Verification
- `docker compose up -d --build lala-web`
- Visit `/`, `/signin`, `/dashboard` — all pages render correctly
- Header auth button only visible on search page in multi-tenant mode
- `curl http://localhost:8081/includes/header.html` returns 404 (internal only)

---

## Commit 2: TypeScript with esbuild

### Files to create

**`lala-web/package.json`**:
```json
{
  "name": "lala-web",
  "private": true,
  "scripts": {
    "build": "esbuild src/pages/*.ts --outdir=html/js --bundle --minify --target=es2020",
    "watch": "esbuild src/pages/*.ts --outdir=html/js --bundle --sourcemap --target=es2020 --watch",
    "typecheck": "tsc --noEmit"
  },
  "devDependencies": {
    "esbuild": "^0.25.0",
    "typescript": "^5.7.0"
  }
}
```

**`lala-web/tsconfig.json`** (consistent with `tests/e2e/tsconfig.json` conventions):
```json
{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true,
    "noEmit": true,
    "isolatedModules": true
  },
  "include": ["src/**/*.ts"],
  "exclude": ["node_modules"]
}
```

**`lala-web/src/types/alpine.d.ts`** — minimal type declarations (no `@types/alpinejs` dependency)

**`lala-web/src/lib/api.ts`** — shared helpers extracted from duplicated code:
- `fetchCurrentUser()` — the `/api/auth/me` check (used by all 3 pages)
- `fetchDeploymentMode()` — the `/api/version` check (search page)
- `formatDate()` — date formatting (dashboard)

**`lala-web/src/pages/search.ts`** — `pageApp()` + `searchApp()`, registered on `window`
**`lala-web/src/pages/signin.ts`** — `signInPage()`, registered on `window`
**`lala-web/src/pages/dashboard.ts`** — `dashboardPage()` + `inviteSection()` + `domainsSection()`, registered on `window`

Each page file ends with `(window as Record<string, unknown>).functionName = functionName;` to make Alpine.js `x-data="functionName()"` work.

### Files to modify

**All 3 HTML pages** — replace inline `<script>` blocks with external bundles:
```html
<head>
    <!--#include virtual="/includes/head.html" -->
    <title>...</title>
    <script defer src="/js/search.js"></script>
    <script defer src="https://cdn.jsdelivr.net/npm/alpinejs@3.x.x/dist/cdn.min.js"></script>
</head>
```

Script ordering: page bundle `defer` runs before Alpine CDN `defer` (both defer, document order), so component functions are defined before Alpine initializes.

**`lala-web/Dockerfile`** — multi-stage build:
```dockerfile
# Stage 1: Build TypeScript
FROM node:22-alpine AS build
WORKDIR /build
COPY package.json package-lock.json ./
RUN npm ci
COPY tsconfig.json ./
COPY src/ ./src/
RUN npm run build

# Stage 2: Serve with nginx
FROM nginx:alpine
COPY html/ /usr/share/nginx/html/
COPY --from=build /build/html/js/ /usr/share/nginx/html/js/
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
HEALTHCHECK --interval=10s --timeout=3s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost/health || exit 1
CMD ["nginx", "-g", "daemon off;"]
```

**`.gitignore`** — add:
```
lala-web/node_modules/
lala-web/html/js/
```

### Development workflow
- Run `npm run watch` in `lala-web/` — esbuild watches and outputs to `html/js/`
- `html/js/` is volume-mounted into Docker, so changes appear on refresh
- SSI includes are also volume-mounted — HTML changes also appear on refresh
- `npm run typecheck` for full type checking (esbuild skips types for speed)

### Verification
- `cd lala-web && npm install && npm run build && npm run typecheck`
- `docker compose up -d --build lala-web`
- Visit all 3 pages — functionality identical to before
- Run E2E tests if available

## Final structure

```
lala-web/
├── html/
│   ├── index.html              (SSI includes, external JS ref)
│   ├── styles.css              (unchanged)
│   ├── includes/
│   │   ├── head.html           (meta + CSS)
│   │   ├── header.html         (logo + tagline + auth btn)
│   │   └── footer.html         (copyright)
│   ├── js/                     (gitignored, built by esbuild)
│   ├── signin/index.html
│   └── dashboard/index.html
├── src/
│   ├── types/alpine.d.ts
│   ├── lib/api.ts
│   └── pages/{search,signin,dashboard}.ts
├── package.json
├── package-lock.json
├── tsconfig.json
├── nginx.conf                  (ssi on + internal includes)
└── Dockerfile                  (multi-stage node+nginx)
```
