/**
 * Shared API fetch helpers used across multiple pages.
 */

/**
 * Fetch the current user session from /api/auth/me.
 * Returns the user object if authenticated, null otherwise.
 */
export async function fetchCurrentUser(): Promise<Record<string, unknown> | null> {
  try {
    const res = await fetch('/api/auth/me', { credentials: 'include' });
    if (res.ok) {
      return await res.json();
    }
  } catch {
    // Not authenticated or network error
  }
  return null;
}

/**
 * Fetch deployment mode from /api/version.
 * Returns the deployment_mode string if available, null otherwise.
 */
export async function fetchDeploymentMode(): Promise<string | null> {
  try {
    const res = await fetch('/api/version');
    if (res.ok) {
      const data = await res.json();
      return data.deployment_mode ?? null;
    }
  } catch {
    // Version endpoint unavailable
  }
  return null;
}

/**
 * Format a date string for display.
 * Returns an em-dash for null/invalid values.
 */
export function formatDate(dateStr: string | null | undefined): string {
  if (!dateStr) return '\u2014';
  try {
    return new Date(dateStr).toLocaleDateString();
  } catch {
    return dateStr;
  }
}
