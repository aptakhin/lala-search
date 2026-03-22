import { fetchCurrentUser } from '../lib/api';

const SIGNIN_EMAIL_STORAGE_KEY = 'lala-signin-email';

function loadRememberedEmail(): string {
  try {
    return window.sessionStorage.getItem(SIGNIN_EMAIL_STORAGE_KEY) || '';
  } catch {
    return '';
  }
}

function rememberEmail(email: string) {
  try {
    if (!email) {
      window.sessionStorage.removeItem(SIGNIN_EMAIL_STORAGE_KEY);
      return;
    }
    window.sessionStorage.setItem(SIGNIN_EMAIL_STORAGE_KEY, email);
  } catch {
    // Non-critical
  }
}

function signInPage() {
  return {
    email: loadRememberedEmail(),
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

    onEmailInput() {
      rememberEmail(this.email.trim());
    },

    async requestLink() {
      const email = this.email.trim();
      if (!email) {
        this.error = 'Please enter your email address.';
        return;
      }

      this.email = email;
      this.sending = true;
      this.error = null;
      rememberEmail(email);

      try {
        const response = await fetch('/api/auth/request-link', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ email }),
        });

        const data = await response.json();
        if (response.ok && data.success) {
          this.sent = true;
        } else {
          this.sent = false;
          this.error = data.message || 'Failed to send sign-in email.';
        }
      } catch {
        this.sent = false;
        this.error = 'Network error. Is the server running?';
      } finally {
        this.sending = false;
      }
    },
  };
}

window.signInPage = signInPage;
