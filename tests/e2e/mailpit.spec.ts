import { expect, test } from "@playwright/test";

import { extractMagicLinkToken } from "./helpers/inbox";

test("extract_magic_link_token_returns_token_from_message_text", async () => {
  const token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
  const message = {
    Text: `Click here: http://localhost:3000/auth/verify/${token}`,
  };

  expect(extractMagicLinkToken(message)).toBe(token);
});
