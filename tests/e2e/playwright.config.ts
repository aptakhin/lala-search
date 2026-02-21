import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.ts",
  timeout: 120_000,
  retries: 0,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL: process.env.LALA_AGENT_URL || "http://localhost:3000",
    extraHTTPHeaders: {
      "Content-Type": "application/json",
    },
  },
});
