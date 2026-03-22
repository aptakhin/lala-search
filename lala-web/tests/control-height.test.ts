import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

const styles = readFileSync(new URL('../html/styles.css', import.meta.url), 'utf8');
const dashboardHtml = readFileSync(
  new URL('../html/dashboard/index.html', import.meta.url),
  'utf8',
);
const onboardingHtml = readFileSync(
  new URL('../html/onboarding/index.html', import.meta.url),
  'utf8',
);

test('test_inline_form_controls_share_one_line_height_contract', () => {
  assert.match(
    styles,
    /\.inline-form input,\s*\n\.inline-form select,\s*\n\.inline-form button \{/,
  );
  assert.match(styles, /min-height:\s*32px;/);
});

test('test_tenant_name_status_indicator_reserves_space', () => {
  assert.match(styles, /\.input-status-indicator\s*\{/);
  assert.match(styles, /width:\s*20px;/);
  assert.match(styles, /justify-content:\s*center;/);
  assert.match(dashboardHtml, /class="input-status-indicator"/);
  assert.match(onboardingHtml, /class="input-status-indicator"/);
});
