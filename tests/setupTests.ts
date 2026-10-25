import "@testing-library/jest-dom";
import { afterAll, afterEach, beforeAll, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { server } from "./msw/server";
import { resetProviderState } from "./msw/state";
import "./msw/tauriMocks";
import { setWeb } from "./helpers/web";

// Реальный isWeb в jsdom всегда true (нет __TAURI_INTERNALS__) — для тестов дефолт desktop.
// Остальной adapter (invoke, WebUnsupportedError…) настоящий; web-тесты зовут setWeb(true).
vi.mock("@/lib/api/adapter", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/adapter")>();
  return {
    ...actual,
    get isWeb() {
      return (
        (globalThis as { __TEST_IS_WEB__?: boolean }).__TEST_IS_WEB__ === true
      );
    },
  };
});

beforeAll(async () => {
  server.listen({ onUnhandledRequest: "warn" });
  await i18n.use(initReactI18next).init({
    lng: "zh",
    fallbackLng: "zh",
    resources: {
      zh: { translation: {} },
      en: { translation: {} },
    },
    interpolation: {
      escapeValue: false,
    },
  });
});

afterEach(() => {
  cleanup();
  setWeb(false);
  // 页面数据的本地缓存（src/lib/localCache.ts）和用量页记住的选择不能从一个用例带到下一个
  for (const key of Object.keys(localStorage)) {
    if (
      key.startsWith("cc-switch-cache:") ||
      key.startsWith("cc-switch.usage.")
    )
      localStorage.removeItem(key);
  }
  resetProviderState();
  server.resetHandlers();
  vi.clearAllMocks();
});

afterAll(() => {
  server.close();
});
