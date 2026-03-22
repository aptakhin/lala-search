import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

function createContext(overrides = {}) {
  const context = {
    console,
    URL,
    URLSearchParams,
    setTimeout,
    clearTimeout,
    navigator: { platform: 'Win32', clipboard: { writeText() {} } },
    document: { title: 'Dashboard' },
    CustomEvent: class CustomEvent {
      constructor(type, init = {}) {
        this.type = type;
        this.detail = init.detail;
      }
    },
    window: {
      location: {
        href: 'http://localhost/dashboard',
        search: '',
        pathname: '/dashboard',
        origin: 'http://localhost',
      },
      history: {
        replaceState() {},
      },
      open() {},
      dispatchEvent() {},
      addEventListener() {},
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

function loadBundle(path, context) {
  const code = readFileSync(path, 'utf8');
  vm.runInNewContext(code, context, { filename: path });
}

async function flushMicrotasks() {
  await Promise.resolve();
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

async function testOnboardingGoToSearchIncludesQuery() {
  const context = createContext({
    document: { title: 'Onboarding' },
    window: {
      location: {
        href: 'http://localhost/onboarding',
        search: '',
        pathname: '/onboarding',
        origin: 'http://localhost',
      },
      history: {
        replaceState() {},
      },
      open() {},
      dispatchEvent() {},
      addEventListener() {},
    },
  });

  loadBundle('./lala-web/html/js/onboarding.js', context);

  const page = context.window.onboardingPage();
  page.searchQuery = 'rust ownership';
  page.goToSearch();

  assert.equal(
    context.window.location.href,
    '/dashboard?q=rust+ownership',
  );
}

async function testDashboardSearchInitHydratesQueryAndFetchesResults() {
  const requests = [];
  const context = createContext({
    window: {
      location: {
        href: 'http://localhost/dashboard?q=rust+ownership',
        search: '?q=rust+ownership',
        pathname: '/dashboard',
        origin: 'http://localhost',
      },
      history: {
        replaceState() {},
      },
      open() {},
      dispatchEvent() {},
      addEventListener() {},
    },
    fetch: async (url, init) => {
      requests.push({
        url: String(url),
        body: typeof init?.body === 'string' ? init.body : null,
      });

      return {
        ok: true,
        async json() {
          return {
            results: [
              {
                document: {
                  url: 'https://doc.rust-lang.org/book/',
                  title: 'The Rust Programming Language',
                },
              },
            ],
            total: 1,
          };
        },
      };
    },
  });

  loadBundle('./lala-web/html/js/dashboard.js', context);

  const search = context.window.dashboardSearch();
  search.$data = { tenantId: 'tenant-123' };

  search.init();
  await flushMicrotasks();

  assert.equal(search.query, 'rust ownership');
  assert.equal(search.results.length, 1);
  assert.equal(search.totalResults, 1);
  assert.equal(requests.length, 1);
  assert.equal(requests[0]?.url, '/api/search?tenant_id=tenant-123');
  assert.match(requests[0]?.body || '', /"query":"rust ownership"/);
}

function testDashboardHtmlInitializesSearchComponent() {
  const html = readFileSync('./lala-web/html/dashboard/index.html', 'utf8');
  assert.match(html, /x-data="dashboardSearch\(\)"\s+x-init="init\(\)"/);
}

await testOnboardingGoToSearchIncludesQuery();
await testDashboardSearchInitHydratesQueryAndFetchesResults();
testDashboardHtmlInitializesSearchComponent();
