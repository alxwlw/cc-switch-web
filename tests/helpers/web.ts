// Тестовый дефолт: isWeb === false (desktop). Web-ветку включает setWeb(true);
// сбрасывается в setupTests.ts после каждого теста.
export function setWeb(value: boolean): void {
  (globalThis as { __TEST_IS_WEB__?: boolean }).__TEST_IS_WEB__ = value;
}
