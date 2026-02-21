import { fetchCurrentUser, formatDate } from '../lib/api';

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

function dashboardPage() {
  return {
    user: null as User | null,
    ready: false,

    get currentOrg(): Organization | null {
      return this.user?.organizations?.[0] || null;
    },

    get tenantId(): string {
      return this.currentOrg?.tenant_id || '';
    },

    get canManage(): boolean {
      const role = this.currentOrg?.role;
      return role === 'owner' || role === 'admin';
    },

    async init() {
      try {
        const res = await fetch('/api/auth/me', { credentials: 'include' });
        if (res.ok) {
          this.user = await res.json();
        } else {
          window.location.href = '/signin';
          return;
        }
      } catch {
        window.location.href = '/signin';
        return;
      }
      this.ready = true;
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
      window.location.href = '/signin';
    },

    formatDate,
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
        const res = await fetch(
          '/api/auth/organizations/' +
            encodeURIComponent(tid) +
            '/members',
          { credentials: 'include' },
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
        const res = await fetch(
          '/api/auth/organizations/' +
            encodeURIComponent(tid) +
            '/invite',
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            credentials: 'include',
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
      if (!confirm('Remove this member?')) return;
      const tid = (this as unknown as AlpineScope).$data.tenantId as string;
      try {
        const res = await fetch(
          '/api/auth/organizations/' +
            encodeURIComponent(tid) +
            '/members/' +
            encodeURIComponent(userId),
          { method: 'DELETE', credentials: 'include' },
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

    async loadDomains() {
      this.domainsLoading = true;
      try {
        const res = await fetch('/api/admin/allowed-domains', {
          credentials: 'include',
        });
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

        const res = await fetch('/api/admin/allowed-domains', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          credentials: 'include',
          body: JSON.stringify(body),
        });
        const data = await res.json();
        if (res.ok && data.success) {
          this.domainMessage = data.message;
          this.newDomain = '';
          this.domainNotes = '';
          await this.loadDomains();
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
      if (!confirm('Remove domain "' + domain + '"?')) return;
      try {
        const res = await fetch(
          '/api/admin/allowed-domains/' + encodeURIComponent(domain),
          { method: 'DELETE', credentials: 'include' },
        );
        if (res.ok) {
          await this.loadDomains();
        }
      } catch {
        // silently fail
      }
    },

    formatDate,
  };
}

window.dashboardPage = dashboardPage;
window.inviteSection = inviteSection;
window.domainsSection = domainsSection;
