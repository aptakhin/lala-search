import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

const styles = readFileSync(new URL('../html/styles.css', import.meta.url), 'utf8');

test('test_inline_form_controls_share_one_line_height_contract', () => {
  assert.match(
    styles,
    /\.inline-form input,\s*\n\.inline-form select,\s*\n\.inline-form button \{/,
  );
  assert.match(styles, /min-height:\s*32px;/);
});
