import { fetchCurrentUser } from '../lib/api';

function signInPage() {
  return {
    email: '',
    sending: false,
    sent: false,
    error: null as string | null,

    async init() {
      // If already authenticated, redirect to dashboard
      const user = await fetchCurrentUser();
      if (user) {
        window.location.href = '/dashboard';
      }
    },

    async requestLink() {
      if (!this.email.trim()) {
        this.error = 'Please enter your email address.';
        return;
      }

      this.sending = true;
      this.error = null;

      try {
        const response = await fetch('/api/auth/request-link', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ email: this.email }),
        });

        const data = await response.json();
        if (response.ok && data.success) {
          this.sent = true;
        } else {
          this.error = data.message || 'Failed to send sign-in email.';
        }
      } catch {
        this.error = 'Network error. Is the server running?';
      } finally {
        this.sending = false;
      }
    },
  };
}

window.signInPage = signInPage;
