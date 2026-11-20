// Test default: isWeb === false (desktop). The web branch is enabled by setWeb(true);
// it is reset in setupTests.ts after every test.
export function setWeb(value: boolean): void {
  (globalThis as { __TEST_IS_WEB__?: boolean }).__TEST_IS_WEB__ = value;
}
