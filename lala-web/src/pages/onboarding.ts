import { formatDate } from '../lib/api';

// Consumer email domains that should show preset domain suggestions
const CONSUMER_DOMAINS = new Set([
  'gmail.com',
  'googlemail.com',
  'yahoo.com',
  'yahoo.co.uk',
  'hotmail.com',
  'outlook.com',
  'live.com',
  'msn.com',
  'aol.com',
  'icloud.com',
  'me.com',
  'mac.com',
  'mail.com',
  'protonmail.com',
  'proton.me',
  'zoho.com',
  'yandex.com',
  'yandex.ru',
  'gmx.com',
  'gmx.net',
]);

// Preset demo domains for consumer email users
const PRESET_DOMAINS = [
  { domain: 'docs.python.org', label: 'Python Docs' },
  { domain: 'developer.mozilla.org', label: 'MDN Web Docs' },
  { domain: 'doc.rust-lang.org', label: 'Rust Docs' },
];

// Common English stopwords to filter from keyword extraction
const STOPWORDS = new Set([
  'the',
  'a',
  'an',
  'is',
  'are',
  'was',
  'were',
  'be',
  'been',
  'being',
  'have',
  'has',
  'had',
  'do',
  'does',
  'did',
  'will',
  'would',
  'could',
  'should',
  'may',
  'might',
  'shall',
  'can',
  'to',
  'of',
  'in',
  'for',
  'on',
  'with',
  'at',
  'by',
  'from',
  'as',
  'into',
  'through',
  'during',
  'before',
  'after',
  'above',
  'below',
  'between',
  'and',
  'but',
  'or',
  'nor',
  'not',
  'so',
  'this',
  'that',
  'these',
  'those',
  'it',
  'its',
  'he',
  'she',
  'they',
  'them',
  'we',
  'you',
  'me',
  'my',
  'your',
  'his',
  'her',
  'our',
  'their',
  'what',
  'which',
  'who',
  'when',
  'where',
  'how',
  'all',
  'each',
  'every',
  'both',
  'few',
  'more',
  'most',
  'other',
  'some',
  'such',
  'no',
  'only',
  'own',
  'same',
  'than',
  'too',
  'very',
  'just',
  'about',
  'up',
  'out',
  'if',
  'then',
]);

/** Extract top keywords from a text string */
function extractKeywords(text: string, maxKeywords = 5): string[] {
  const words = text
    .toLowerCase()
    .replace(/[^a-z0-9\s]/g, '')
    .split(/\s+/);
  const freq: Record<string, number> = {};
  for (const word of words) {
    if (word.length < 3 || STOPWORDS.has(word)) continue;
    freq[word] = (freq[word] || 0) + 1;
  }
  return Object.entries(freq)
    .sort((a, b) => b[1] - a[1])
    .slice(0, maxKeywords)
    .map(([word]) => word);
}

/** Format bytes into human-readable size */
function formatBytes(bytes: number): string {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
  return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
}

interface User {
  user_id: string;
  email: string;
  organizations?: Array<{ tenant_id: string; name?: string; role: string }>;
}

interface RecentPage {
  url: string;
  http_status: number;
  content_length: number;
  last_crawled_at: number;
  title?: string | null;
  excerpt?: string | null;
  keywords?: string[];
}

function onboardingPage() {
  return {
    user: null as User | null,
    ready: false,

    // Domain suggestion state
    suggestedDomain: '',
    isConsumerEmail: false,
    presetDomains: PRESET_DOMAINS,
    selectedDomain: '',

    // Crawl progress state
    domainAdded: false,
    addingDomain: false,
    addError: null as string | null,
    polling: false,
    pollCount: 0,
    maxPolls: 5,
    recentPages: [] as RecentPage[],
    totalCrawled: 0,
    enriched: false,

    // Existing index state
    hasExistingDocs: false,
    existingDomain: '',
    indexHistory: [] as RecentPage[],
    suggestedKeywords: [] as string[],

    // Search trial state
    searchQuery: '',
    orgName: '',

    async init() {
      // Check authentication
      try {
        const res = await fetch('/api/auth/me', { credentials: 'include' });
        if (!res.ok) {
          window.location.href = '/signin';
          return;
        }
        this.user = await res.json();
        const org = this.user?.organizations?.[0];
        if (org?.name) {
          this.orgName = org.name;
        }
      } catch {
        window.location.href = '/signin';
        return;
      }

      this.suggestDomain();
      await this.checkExistingDocs();
      this.ready = true;
    },

    async checkExistingDocs() {
      try {
        const res = await fetch('/api/admin/allowed-domains', {
          credentials: 'include',
        });
        if (!res.ok) return;

        const data = await res.json();
        const domains: Array<{ domain: string }> = data.domains || data || [];
        if (domains.length === 0) return;

        this.existingDomain = domains[0].domain;
        await this.fetchIndexHistory();
      } catch {
        // Non-critical
      }
    },

    async fetchIndexHistory() {
      try {
        const res = await fetch(
          '/api/admin/crawled-pages/recent?domain=' +
            encodeURIComponent(this.existingDomain) +
            '&limit=5&enrich=true',
          { credentials: 'include' },
        );
        if (!res.ok) return;

        const data = await res.json();
        if (!data.pages || data.pages.length === 0) return;

        this.hasExistingDocs = true;
        this.indexHistory = data.pages.map((page: RecentPage) => ({
          ...page,
          keywords: extractKeywords(
            (page.title || '') + ' ' + (page.excerpt || ''),
            5,
          ),
        }));

        // Collect top keywords across all pages for search suggestions
        const allKeywords: Record<string, number> = {};
        for (const page of this.indexHistory) {
          for (const kw of page.keywords || []) {
            allKeywords[kw] = (allKeywords[kw] || 0) + 1;
          }
        }
        this.suggestedKeywords = Object.entries(allKeywords)
          .sort((a, b) => b[1] - a[1])
          .slice(0, 6)
          .map(([word]) => word);
      } catch {
        // Non-critical
      }
    },

    fillSuggestion(keyword: string) {
      this.searchQuery = keyword;
    },

    goToSearch() {
      if (!this.searchQuery.trim()) return;
      window.location.href = '/?q=' + encodeURIComponent(this.searchQuery.trim());
    },

    suggestDomain() {
      if (!this.user?.email) return;
      const emailDomain = this.user.email.split('@')[1];
      if (!emailDomain) return;

      if (CONSUMER_DOMAINS.has(emailDomain.toLowerCase())) {
        this.isConsumerEmail = true;
        this.selectedDomain = PRESET_DOMAINS[0].domain;
      } else {
        this.isConsumerEmail = false;
        this.suggestedDomain = emailDomain;
        this.selectedDomain = emailDomain;
      }
    },

    selectPreset(domain: string) {
      this.selectedDomain = domain;
    },

    async addDomainAndStartCrawl() {
      if (!this.selectedDomain.trim()) {
        this.addError = 'Please select or enter a domain.';
        return;
      }

      this.addingDomain = true;
      this.addError = null;

      try {
        // Step 1: Add domain to allowed domains
        const addRes = await fetch('/api/admin/allowed-domains', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          credentials: 'include',
          body: JSON.stringify({
            domain: this.selectedDomain,
            notes: 'Added during onboarding',
          }),
        });

        if (!addRes.ok) {
          const data = await addRes.json();
          this.addError = data.message || 'Failed to add domain.';
          return;
        }

        // Step 2: Queue the root URL for crawling
        const queueRes = await fetch('/api/queue/add', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          credentials: 'include',
          body: JSON.stringify({
            url: 'https://' + this.selectedDomain + '/',
            priority: 0,
          }),
        });

        if (!queueRes.ok) {
          const data = await queueRes.json();
          this.addError = data.message || 'Failed to queue URL.';
          return;
        }

        this.domainAdded = true;
        this.startPolling();
      } catch {
        this.addError = 'Network error. Please try again.';
      } finally {
        this.addingDomain = false;
      }
    },

    startPolling() {
      this.polling = true;
      this.pollCount = 0;
      this.pollForPages();
    },

    async pollForPages() {
      if (this.pollCount >= this.maxPolls) {
        this.polling = false;
        this.enrichPages();
        return;
      }

      // Wait 2 seconds between polls
      await new Promise((resolve) => setTimeout(resolve, 2000));
      this.pollCount++;

      try {
        const res = await fetch(
          '/api/admin/crawled-pages/recent?domain=' +
            encodeURIComponent(this.selectedDomain) +
            '&limit=10',
          { credentials: 'include' },
        );

        if (res.ok) {
          const data = await res.json();
          this.totalCrawled = data.total || 0;
          // Only update if we got new pages
          if (data.pages && data.pages.length > 0) {
            this.recentPages = data.pages.map((page: RecentPage) => ({
              ...page,
              keywords: [],
            }));
          }
        }
      } catch {
        // Polling failure is non-critical
      }

      // Continue polling
      if (this.pollCount < this.maxPolls) {
        this.pollForPages();
      } else {
        this.polling = false;
        this.enrichPages();
      }
    },

    async enrichPages() {
      if (this.enriched || this.recentPages.length === 0) return;
      this.enriched = true;

      try {
        const res = await fetch(
          '/api/admin/crawled-pages/recent?domain=' +
            encodeURIComponent(this.selectedDomain) +
            '&limit=10&enrich=true',
          { credentials: 'include' },
        );

        if (!res.ok) return;

        const data = await res.json();
        if (!data.pages) return;

        this.totalCrawled = data.total || this.totalCrawled;
        this.recentPages = data.pages.map((page: RecentPage) => ({
          ...page,
          keywords: extractKeywords(
            (page.title || '') + ' ' + (page.excerpt || ''),
            5,
          ),
        }));

        // Build search suggestions from crawled pages
        const allKeywords: Record<string, number> = {};
        for (const page of this.recentPages) {
          for (const kw of page.keywords || []) {
            allKeywords[kw] = (allKeywords[kw] || 0) + 1;
          }
        }
        this.suggestedKeywords = Object.entries(allKeywords)
          .sort((a, b) => b[1] - a[1])
          .slice(0, 6)
          .map(([word]) => word);
      } catch {
        // Enrichment failure is non-critical
      }
    },

    goToDashboard() {
      window.location.href = '/dashboard';
    },

    formatBytes,
    formatDate,
  };
}

window.onboardingPage = onboardingPage;
