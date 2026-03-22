export type SearchShortcutAction = 'submit' | 'newline' | 'ignore';

interface SearchShortcutEventLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  isComposing?: boolean;
}

export function isApplePlatform(platform: string | null | undefined): boolean {
  return /mac|iphone|ipad|ipod/i.test(platform ?? '');
}

export function getSearchShortcutHint(
  platform: string | null | undefined,
): string {
  const modifier = isApplePlatform(platform) ? 'Cmd' : 'Ctrl';
  return `Enter to search. ${modifier}+Enter for a new line.`;
}

export function getSearchShortcutAction(
  platform: string | null | undefined,
  event: SearchShortcutEventLike,
): SearchShortcutAction {
  if (event.isComposing || event.key !== 'Enter') {
    return 'ignore';
  }

  const isNewlineShortcut = isApplePlatform(platform)
    ? event.metaKey
    : event.ctrlKey;

  return isNewlineShortcut ? 'newline' : 'submit';
}

export function getClientPlatform(): string {
  return globalThis.navigator?.platform ?? '';
}
