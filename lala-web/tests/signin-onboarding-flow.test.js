import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const WEB_ROOT = path.resolve(__dirname, '..');

function webPath(...segments) {
  return path.join(WEB_ROOT, ...segments);
}

function createSessionStorage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem(key) {
      return values.has(key) ? values.get(key) : null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
}

function createContext(overrides = {}) {
  const sessionStorage = overrides.sessionStorage || createSessionStorage();
  const context = {
    console,
    URL,
    URLSearchParams,
    setTimeout,
    clearTimeout,
    navigator: { platform: 'Win32' },
    document: {
      title: 'LalaSearch',
      addEventListener() {},
    },
    sessionStorage,
    window: {
      location: {
        href: 'http://localhost/signin',
        search: '',
        pathname: '/signin',
        origin: 'http://localhost',
      },
      history: {
        replaceState() {},
      },
      sessionStorage,
      dispatchEvent() {},
      addEventListener() {},
      open() {},
    },
    fetch: async () => ({
      ok: true,
      async json() {
        return {};
      },
    }),
    ...overrides,
  };

  context.window.window = context.window;
  context.window.document = context.document;
  context.window.navigator = context.navigator;
  context.window.fetch = context.fetch;
  context.globalThis = context;
  return context;
}

function loadBundle(bundlePath, context) {
  const code = readFileSync(bundlePath, 'utf8');
  vm.runInNewContext(code, context, { filename: bundlePath });
}

async function flushMicrotasks() {
  await Promise.resolve();
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

async function testDashboardKeepsExplicitOnboardingNavigation() {
  const requests = [];
  const context = createContext({
    window: {
      location: {
        href: 'http://localhost/dashboard?from=onboarding',
        search: '?from=onboarding',
        pathname: '/dashboard',
        origin: 'http://localhost',
      },
      history: {
        replaceState() {},
      },
      dispatchEvent() {},
      addEventListener() {},
      open() {},
    },
    fetch: async (url) => {
      requests.push(String(url));

      if (String(url) === '/api/auth/me') {
        return {
          ok: true,
          async json() {
            return {
              user_id: 'user-1',
              email: 'owner@example.com',
              organizations: [{ tenant_id: 'tenant-1', role: 'owner' }],
            };
          },
        };
      }

      if (String(url) === '/api/version') {
        return {
          ok: true,
          async json() {
            return { deployment_mode: null };
          },
        };
      }

      if (String(url) === '/api/admin/allowed-domains?tenant_id=tenant-1') {
        return {
          ok: true,
          async json() {
            return { domains: [] };
          },
        };
      }

      return {
        ok: true,
        async json() {
          return {};
        },
      };
    },
  });

  loadBundle(webPath('html', 'js', 'dashboard.js'), context);

  const page = context.window.dashboardPage();
  await page.init();
  await flushMicrotasks();

  assert.equal(page.ready, true);
  assert.equal(context.window.location.href, 'http://localhost/dashboard?from=onboarding');
  assert.ok(requests.includes('/api/admin/allowed-domains?tenant_id=tenant-1'));
}

async function testSignInRestoresEmailAndResendsSameAddress() {
  const requests = [];
  const context = createContext({
    sessionStorage: createSessionStorage({
      'lala-signin-email': 'saved@example.com',
    }),
    fetch: async (url, init) => {
      requests.push({
        url: String(url),
        body: typeof init?.body === 'string' ? init.body : null,
      });

      if (String(url) === '/api/auth/me') {
        return {
          ok: false,
          async json() {
            return {};
          },
        };
      }

      if (String(url) === '/api/auth/request-link') {
        return {
          ok: true,
          async json() {
            return { success: true };
          },
        };
      }

      return {
        ok: true,
        async json() {
          return {};
        },
      };
    },
  });

  loadBundle(webPath('html', 'js', 'signin.js'), context);

  const page = context.window.signInPage();
  await page.init();
  await flushMicrotasks();

  assert.equal(page.email, 'saved@example.com');

  await page.requestLink();

  assert.equal(page.sent, true);
  assert.equal(requests.at(-1)?.url, '/api/auth/request-link');
  assert.match(requests.at(-1)?.body || '', /"email":"saved@example\.com"/);
  assert.equal(
    context.sessionStorage.getItem('lala-signin-email'),
    'saved@example.com',
  );
}

function testSignInHtmlResendsInsteadOfClearingEmail() {
  const html = readFileSync(webPath('html', 'signin', 'index.html'), 'utf8');
  assert.match(
    html,
    /<button @click="requestLink\(\)"/,
  );
  assert.doesNotMatch(
    html,
    /@click="sent = false; email = ''"/,
  );
}

await testDashboardKeepsExplicitOnboardingNavigation();
await testSignInRestoresEmailAndResendsSameAddress();
testSignInHtmlResendsInsteadOfClearingEmail();
