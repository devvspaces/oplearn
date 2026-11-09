import { defineConfig } from "@playwright/test";

// Smoke tests run the built renderer in Chromium with Tauri IPC mocked (tests/e2e/mock-ipc.ts).
// Tauri itself has no WebDriver support on macOS, so native flows are verified manually (see README).
export default defineConfig({
  testDir: "tests/e2e",
  use: { baseURL: "http://localhost:4173" },
  webServer: {
    command: "pnpm build && pnpm preview",
    url: "http://localhost:4173",
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
