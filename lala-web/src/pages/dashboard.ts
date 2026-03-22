import { fetchCurrentUser, fetchDeploymentMode, formatDate } from '../lib/api';
import {
  getClientPlatform,
  getSearchShortcutAction,
  getSearchShortcutHint,
} from '../lib/search-input';

interface User {
  user_id: string;
  email: string;
  organizations?: Array<{
    tenant_id: string;
    role: string;
  }>;
}

interface Organization {
  tenant_id: string;
  name?: string;
  role: string;
}

interface Member {
  user_id: string;
  email: string;
  role: string;
  joined_at: string;
}

interface Domain {
  domain: string;
  added_by: string;
  notes: string | null;
  added_at: string;
}

interface ActionRecord {
  action_id: string;
  entity_type: string;
  action_type: string;
  entity_id: string;
  description: string;
  performed_at: string;
  rolled_back_at: string | null;
}

interface IndexCapacityResponse {
  usage_bytes: number;
  max_bytes: number;
  limit_reached: boolean;
  can_edit_max: boolean;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
  if (bytes < 1024 * 1024 * 1024)
    return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
  return (bytes / (1024 * 1024 * 1024)).toFixed(1) + ' GB';
}

function adminFetch(
  tenantId: string,
  url: string,
  options: RequestInit = {},
): Promise<Response> {
  if (tenantId) {
    const sep = url.includes('?') ? '&' : '?';
    url = url + sep + 'tenant_id=' + encodeURIComponent(tenantId);
  }
  return fetch(url, { ...options, credentials: 'include' });
}

function shouldStayOnDashboardWithoutDomains(): boolean {
  const params = new URLSearchParams(window.location.search);
  return params.get('from') === 'onboarding';
}

function dashboardPage() {
  let tenantNameTimer: ReturnType<typeof setTimeout> | null = null;

  return {
    user: null as User | null,
    deploymentMode: null as string | null,
    ready: false,
    undoable: null as ActionRecord | null,
    redoable: null as ActionRecord | null,
    actionMessage: null as string | null,
    undoing: false,
    redoing: false,

    // Tenant name
    tenantNameValue: '',
    tenantNameSaving: false,
    tenantNameSaved: false,

    // Index capacity
    indexUsageBytes: 0,
    indexMaxBytes: 0,
    indexLimitReached: false,
    indexCanEditMax: false,
    indexCapacityInput: '',
    indexCapacitySaving: false,
    indexCapacitySaved: false,

    // Org switcher
    selectedOrg: null as Organization | null,

    get currentOrg(): Organization | null {
      return this.selectedOrg || this.user?.organizations?.[0] || null;
    },

    get tenantId(): string {
      return this.currentOrg?.tenant_id || '';
    },

    get isOwner(): boolean {
      return this.currentOrg?.role === 'owner';
    },

    get canManage(): boolean {
      const role = this.currentOrg?.role;
      return role === 'owner' || role === 'admin';
    },

    get hasMultipleOrgs(): boolean {
      return (this.user?.organizations?.length || 0) > 1;
    },

    adminFetch(url: string, options: RequestInit = {}): Promise<Response> {
      return adminFetch(this.tenantId, url, options);
    },

    async switchOrg(org: Organization) {
      this.selectedOrg = org;
      this.ready = false;
      await this.reloadDashboard();
      this.ready = true;
    },

    async reloadDashboard() {
      await this.loadUndoRedoState();
      await this.loadTenantName();
      await this.loadIndexCapacity();
      window.dispatchEvent(new CustomEvent('org-switched'));
    },

    async init() {
      const [, deploymentMode] = await Promise.all([
        fetch('/api/auth/me', { credentials: 'include' }).then(async (res) => {
          if (res.ok) {
            this.user = await res.json();
          }
        }),
        fetchDeploymentMode(),
      ]);

      this.deploymentMode = deploymentMode;

      if (!this.user) {
        window.location.href = '/signin';
        return;
      }

      // Redirect to onboarding if no domains are configured yet
      try {
        const domainsRes = await this.adminFetch('/api/admin/allowed-domains');
        if (domainsRes.ok) {
          const data = await domainsRes.json();
          if (
            (!data.domains || data.domains.length === 0) &&
            !shouldStayOnDashboardWithoutDomains()
          ) {
            window.location.href = '/onboarding';
            return;
          }
        }
      } catch {
        // Continue to dashboard if domains check fails
      }

      this.ready = true;
      await this.loadUndoRedoState();
      await this.loadTenantName();
      await this.loadIndexCapacity();

      document.addEventListener('keydown', (e: KeyboardEvent) => {
        if (!(e.ctrlKey || e.metaKey)) return;
        if (e.key === 'z' && !e.shiftKey && this.undoable) {
          e.preventDefault();
          this.undo();
        } else if (
          (e.key === 'y' || (e.key === 'z' && e.shiftKey)) &&
          this.redoable
        ) {
          e.preventDefault();
          this.redo();
        }
      });
    },

    async loadTenantName() {
      try {
        const res = await this.adminFetch('/api/admin/settings/tenant-name');
        if (res.ok) {
          const data = await res.json();
          this.tenantNameValue = data.name || '';
        }
      } catch {
        // Non-critical
      }
    },

    async loadIndexCapacity() {
      try {
        const res = await this.adminFetch('/api/admin/settings/index-capacity');
        if (!res.ok) return;

        const data = (await res.json()) as IndexCapacityResponse;
        this.indexUsageBytes = data.usage_bytes || 0;
        this.indexMaxBytes = data.max_bytes || 0;
        this.indexLimitReached = !!data.limit_reached;
        this.indexCanEditMax = !!data.can_edit_max;
        this.indexCapacityInput = String(data.max_bytes || 0);
      } catch {
        // Non-critical
      }
    },

    onTenantNameInput() {
      this.tenantNameSaved = false;
      if (tenantNameTimer) clearTimeout(tenantNameTimer);
      tenantNameTimer = setTimeout(() => this.saveTenantName(), 600);
    },

    async saveTenantName() {
      const name = this.tenantNameValue.trim();
      if (!name) return;

      this.tenantNameSaving = true;
      this.tenantNameSaved = false;

      try {
        const res = await this.adminFetch('/api/admin/settings/tenant-name', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ name }),
        });
        if (res.ok) {
          this.tenantNameSaved = true;
          setTimeout(() => {
            this.tenantNameSaved = false;
          }, 2000);
        }
      } catch {
        // silently fail
      } finally {
        this.tenantNameSaving = false;
      }
    },

    async saveIndexCapacity() {
      const maxBytes = Number(this.indexCapacityInput);
      if (!Number.isFinite(maxBytes) || maxBytes <= 0) return;

      this.indexCapacitySaving = true;
      this.indexCapacitySaved = false;

      try {
        const res = await this.adminFetch('/api/admin/settings/index-capacity', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ max_bytes: Math.floor(maxBytes) }),
        });
        if (res.ok) {
          await this.loadIndexCapacity();
          this.indexCapacitySaved = true;
          setTimeout(() => {
            this.indexCapacitySaved = false;
          }, 2000);
        }
      } catch {
        // silently fail
      } finally {
        this.indexCapacitySaving = false;
      }
    },

    async loadUndoRedoState() {
      try {
        const res = await this.adminFetch('/api/admin/action-history/state');
        if (res.ok) {
          const data = await res.json();
          this.undoable = data.undoable || null;
          this.redoable = data.redoable || null;
        }
      } catch {
        // silently fail
      }
    },

    async undo() {
      if (!this.undoable || this.undoing) return;
      this.undoing = true;
      this.actionMessage = null;

      try {
        const res = await this.adminFetch('/api/admin/action-history/undo', {
          method: 'POST',
        });
        if (res.ok) {
          const data = await res.json();
          this.actionMessage = data.message;
          window.dispatchEvent(new CustomEvent('action-rolled-back'));
          await this.loadUndoRedoState();
        }
      } catch {
        // silently fail
      } finally {
        this.undoing = false;
      }
    },

    async redo() {
      if (!this.redoable || this.redoing) return;
      this.redoing = true;
      this.actionMessage = null;

      try {
        const res = await this.adminFetch('/api/admin/action-history/redo', {
          method: 'POST',
        });
        if (res.ok) {
          const data = await res.json();
          this.actionMessage = data.message;
          window.dispatchEvent(new CustomEvent('action-rolled-back'));
          await this.loadUndoRedoState();
        }
      } catch {
        // silently fail
      } finally {
        this.redoing = false;
      }
    },

    async signOut() {
      try {
        await fetch('/api/auth/signout', {
          method: 'POST',
          credentials: 'include',
        });
      } catch {
        // Sign out locally regardless
      }
      this.user = null;
      window.location.href = '/';
    },

    formatDate,
    formatBytes,
  };
}

function inviteSection() {
  return {
    inviteEmail: '',
    inviteRole: 'member',
    inviting: false,
    inviteMessage: null as string | null,
    inviteError: null as string | null,
    members: [] as Member[],
    membersLoading: false,

    async loadMembers() {
      const tid = (this as unknown as AlpineScope).$data.tenantId as string;
      if (!tid) return;
      this.membersLoading = true;
      try {
        const res = await adminFetch(
          tid,
          '/api/auth/organizations/' +
            encodeURIComponent(tid) +
            '/members',
        );
        if (res.ok) {
          const data = await res.json();
          this.members = data.members || [];
        }
      } catch {
        // silently fail
      } finally {
        this.membersLoading = false;
      }
    },

    async invite() {
      if (!this.inviteEmail.trim()) {
        this.inviteError = 'Please enter an email address.';
        return;
      }

      this.inviting = true;
      this.inviteError = null;
      this.inviteMessage = null;
      const tid = (this as unknown as AlpineScope).$data.tenantId as string;

      try {
        const res = await adminFetch(
          tid,
          '/api/auth/organizations/' +
            encodeURIComponent(tid) +
            '/invite',
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              email: this.inviteEmail,
              role: this.inviteRole,
            }),
          },
        );
        const data = await res.json();
        if (res.ok && data.success) {
          this.inviteMessage = data.message;
          this.inviteEmail = '';
          this.inviteRole = 'member';
        } else {
          this.inviteError = data.message || 'Failed to send invitation.';
        }
      } catch {
        this.inviteError = 'Network error.';
      } finally {
        this.inviting = false;
      }
    },

    async removeMember(userId: string) {
      const tid = (this as unknown as AlpineScope).$data.tenantId as string;
      try {
        const res = await adminFetch(
          tid,
          '/api/auth/organizations/' +
            encodeURIComponent(tid) +
            '/members/' +
            encodeURIComponent(userId),
          { method: 'DELETE' },
        );
        if (res.ok) {
          await this.loadMembers();
        }
      } catch {
        // silently fail
      }
    },

    formatDate,
  };
}

function domainsSection() {
  return {
    newDomain: '',
    domainNotes: '',
    adding: false,
    domainMessage: null as string | null,
    domainError: null as string | null,
    domains: [] as Domain[],
    domainsLoading: false,

    getTenantId(): string {
      return (this as unknown as AlpineScope).$data.tenantId as string;
    },

    async loadDomains() {
      this.domainsLoading = true;
      try {
        const res = await adminFetch(this.getTenantId(), '/api/admin/allowed-domains');
        if (res.ok) {
          const data = await res.json();
          this.domains = data.domains || [];
        }
      } catch {
        // silently fail
      } finally {
        this.domainsLoading = false;
      }
    },

    async addDomain() {
      if (!this.newDomain.trim()) {
        this.domainError = 'Please enter a domain.';
        return;
      }

      this.adding = true;
      this.domainError = null;
      this.domainMessage = null;

      try {
        const body: { domain: string; notes?: string } = {
          domain: this.newDomain,
        };
        if (this.domainNotes.trim()) {
          body.notes = this.domainNotes;
        }

        const res = await adminFetch(this.getTenantId(), '/api/admin/allowed-domains', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
        const data = await res.json();
        if (res.ok && data.success) {
          this.domainMessage = data.message;
          this.newDomain = '';
          this.domainNotes = '';
          await this.loadDomains();
          const page = (this as unknown as AlpineScope).$data as ReturnType<typeof dashboardPage>;
          if (page.loadUndoRedoState) await page.loadUndoRedoState();
        } else {
          this.domainError = data.message || 'Failed to add domain.';
        }
      } catch {
        this.domainError = 'Network error.';
      } finally {
        this.adding = false;
      }
    },

    async deleteDomain(domain: string) {
      try {
        const res = await adminFetch(
          this.getTenantId(),
          '/api/admin/allowed-domains/' + encodeURIComponent(domain),
          { method: 'DELETE' },
        );
        if (res.ok) {
          await this.loadDomains();
          const page = (this as unknown as AlpineScope).$data as ReturnType<typeof dashboardPage>;
          if (page.loadUndoRedoState) await page.loadUndoRedoState();
        }
      } catch {
        // silently fail
      }
    },

    formatDate,
  };
}

interface SearchResult {
  document: {
    url: string;
    title?: string;
    excerpt?: string;
  };
  snippet?: string;
}

export function dashboardSearch() {
  return {
    query: '',
    results: [] as SearchResult[],
    totalResults: 0,
    isLoading: false,
    hasSearched: false,
    error: null as string | null,
    currentOffset: 0,
    limit: 10,
    platform: getClientPlatform(),

    init() {
      const params = new URLSearchParams(window.location.search);
      const query = params.get('q')?.trim();
      if (!query) {
        return;
      }

      this.query = query;
      void this.search();
    },

    getTenantId(): string {
      return (this as unknown as AlpineScope).$data.tenantId as string;
    },

    get searchShortcutHint() {
      return getSearchShortcutHint(this.platform);
    },

    onQueryKeydown(event: KeyboardEvent) {
      const action = getSearchShortcutAction(this.platform, event);
      if (action !== 'submit') {
        return;
      }

      event.preventDefault();
      this.search();
    },

    async search() {
      const query = this.query.trim();
      if (!query) {
        this.error = 'Please enter a search query.';
        return;
      }

      this.query = query;
      this.isLoading = true;
      this.error = null;
      this.currentOffset = 0;
      this.hasSearched = true;

      const url = new URL(window.location.href);
      url.searchParams.set('q', query);
      window.history.replaceState({}, '', url.toString());

      try {
        const res = await adminFetch(this.getTenantId(), '/api/search', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            query,
            limit: this.limit,
            offset: this.currentOffset,
          }),
        });

        if (!res.ok) {
          throw new Error('Server error: ' + res.status);
        }

        const data = await res.json();
        this.results = data.results || [];
        this.totalResults = data.total || 0;
      } catch (err: unknown) {
        this.error =
          (err instanceof Error ? err.message : null) ||
          'Failed to fetch results.';
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
        const res = await adminFetch(this.getTenantId(), '/api/search', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            query: this.query,
            limit: this.limit,
            offset: this.currentOffset,
          }),
        });

        if (!res.ok) {
          throw new Error('Server error: ' + res.status);
        }

        const data = await res.json();
        this.results = data.results || [];
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
  };
}

if (typeof window !== 'undefined') {
  window.dashboardPage = dashboardPage;
  window.inviteSection = inviteSection;
  window.domainsSection = domainsSection;
  window.dashboardSearch = dashboardSearch;
}
