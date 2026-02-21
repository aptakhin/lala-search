import { fetchCurrentUser, fetchDeploymentMode } from '../lib/api';

interface User {
  email: string;
  organizations?: Array<{ tenant_id: string; role: string }>;
}

interface SearchResult {
  document: {
    url: string;
    title?: string;
    excerpt?: string;
  };
  snippet?: string;
}

function pageApp() {
  return {
    user: null as User | null,
    deploymentMode: null as string | null,
    ready: false,

    async init() {
      const [deploymentMode, user] = await Promise.allSettled([
        fetchDeploymentMode(),
        fetchCurrentUser(),
      ]);

      if (deploymentMode.status === 'fulfilled' && deploymentMode.value) {
        this.deploymentMode = deploymentMode.value;
      }

      if (user.status === 'fulfilled' && user.value) {
        this.user = user.value as unknown as User;
      }

      this.ready = true;

      // Single-tenant: redirect away from search page
      if (this.deploymentMode === 'single_tenant') {
        if (this.user) {
          window.location.href = '/dashboard';
        } else {
          window.location.href = '/signin';
        }
      }
    },
  };
}

function searchApp() {
  return {
    query: '',
    results: [] as SearchResult[],
    totalResults: 0,
    isLoading: false,
    hasSearched: false,
    error: null as string | null,
    currentOffset: 0,
    limit: 10,

    init() {
      const params = new URLSearchParams(window.location.search);
      const q = params.get('q');
      if (q) {
        this.query = q;
        this.search();
      }
    },

    async search() {
      if (!this.query.trim()) {
        this.error = 'Please enter a search query!';
        return;
      }

      this.isLoading = true;
      this.error = null;
      this.currentOffset = 0;
      this.hasSearched = true;

      try {
        const response = await fetch('/api/search', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          credentials: 'include',
          body: JSON.stringify({
            query: this.query,
            limit: this.limit,
            offset: this.currentOffset,
          }),
        });

        if (!response.ok) {
          throw new Error('Server error: ' + response.status);
        }

        const data = await response.json();
        this.results = data.results || [];
        this.totalResults = data.total || 0;
      } catch (err: unknown) {
        this.error =
          (err instanceof Error ? err.message : null) ||
          'Failed to fetch results. Is the backend running?';
        this.results = [];
        this.totalResults = 0;
      } finally {
        this.isLoading = false;
      }
    },

    clearSearch() {
      this.query = '';
      this.results = [];
      this.totalResults = 0;
      this.hasSearched = false;
      this.error = null;
      this.currentOffset = 0;
      window.history.replaceState({}, document.title, window.location.pathname);
    },

    nextPage() {
      this.currentOffset += this.limit;
      this.performPaginatedSearch();
    },

    previousPage() {
      this.currentOffset = Math.max(0, this.currentOffset - this.limit);
      this.performPaginatedSearch();
    },

    async performPaginatedSearch() {
      this.isLoading = true;
      this.error = null;

      try {
        const response = await fetch('/api/search', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          credentials: 'include',
          body: JSON.stringify({
            query: this.query,
            limit: this.limit,
            offset: this.currentOffset,
          }),
        });

        if (!response.ok) {
          throw new Error('Server error: ' + response.status);
        }

        const data = await response.json();
        this.results = data.results || [];
        window.scrollTo({ top: 0, behavior: 'smooth' });
      } catch (err: unknown) {
        this.error =
          (err instanceof Error ? err.message : null) ||
          'Failed to fetch results.';
        this.results = [];
      } finally {
        this.isLoading = false;
      }
    },

    openUrl(url: string) {
      window.open(url, '_blank');
    },

    copyToClipboard(text: string) {
      navigator.clipboard.writeText(text);
    },
  };
}

// Register as globals for Alpine.js x-data="functionName()" pattern
window.pageApp = pageApp;
window.searchApp = searchApp;
