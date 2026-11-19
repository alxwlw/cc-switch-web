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

// A request without a handler fails the test that made it, instead of only
// logging while the component quietly takes its error path.
const unhandledRequests: string[] = [];

beforeAll(async () => {
  server.listen({
    onUnhandledRequest: (request) => {
      unhandledRequests.push(`${request.method} ${request.url}`);
    },
  });
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
  // Object.keys(localStorage) is empty for the plain-object polyfill of tests/setupGlobals.ts, so walk key(i)
  const storedKeys = Array.from({ length: localStorage.length }, (_, i) =>
    localStorage.key(i),
  );
  for (const key of storedKeys) {
    if (
      key &&
      (key.startsWith("cc-switch-cache:") || key.startsWith("cc-switch.usage."))
    )
      localStorage.removeItem(key);
  }
  resetProviderState();
  server.resetHandlers();
  vi.clearAllMocks();
  const unhandled = unhandledRequests.splice(0);
  if (unhandled.length > 0) {
    throw new Error(
      `Requests without an MSW handler (add one to tests/msw/handlers.ts or server.use it in the test):\n${unhandled.join("\n")}`,
    );
  }
});

afterAll(() => {
  server.close();
});
