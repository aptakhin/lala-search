import assert from 'node:assert/strict';

import {
  getSearchShortcutAction,
  getSearchShortcutHint,
} from '../src/lib/search-input.ts';

function run() {
  const action = getSearchShortcutAction('Win32', {
    key: 'Enter',
    ctrlKey: false,
    metaKey: false,
  });

  assert.equal(action, 'submit');

  const windowsNewlineAction = getSearchShortcutAction('Win32', {
    key: 'Enter',
    ctrlKey: true,
    metaKey: false,
  });

  assert.equal(windowsNewlineAction, 'newline');

  const macNewlineAction = getSearchShortcutAction('MacIntel', {
    key: 'Enter',
    ctrlKey: false,
    metaKey: true,
  });

  assert.equal(macNewlineAction, 'newline');

  const ignoredAction = getSearchShortcutAction('Win32', {
    key: 'a',
    ctrlKey: false,
    metaKey: false,
  });

  assert.equal(ignoredAction, 'ignore');

  assert.equal(
    getSearchShortcutHint('MacIntel'),
    'Enter to search. Cmd+Enter for a new line.',
  );
  assert.equal(
    getSearchShortcutHint('Win32'),
    'Enter to search. Ctrl+Enter for a new line.',
  );
}

run();
