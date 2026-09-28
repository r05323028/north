import { defineConfig } from "@playwright/test";

const baseURL = process.env.NORTH_RELEASE_BASE_URL;
if (!baseURL) {
  throw new Error(
    "NORTH_RELEASE_BASE_URL is required; release E2E must target assembled proxy",
  );
}

export default defineConfig({
  testDir: "./e2e",
  testMatch: "release-qualification.spec.ts",
  fullyParallel: false,
  forbidOnly: true,
  retries: 0,
  reporter: "line",
  use: {
    baseURL,
    storageState: process.env.NORTH_RELEASE_STORAGE_STATE,
    launchOptions: { args: ["--use-system-ca"] },
    trace: "retain-on-failure",
  },
});
