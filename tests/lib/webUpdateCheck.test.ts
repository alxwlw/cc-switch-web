import { describe, expect, it } from "vitest";
import { parseWebUpdateCheck } from "@/lib/updater";

/** The server's `UpdateCheck` of a commit-mode check, over the given fields. */
function raw(over: Record<string, unknown> = {}) {
  return {
    status: "behind",
    currentVersion: "4.0.35-web.1",
    currentSha: "0123456789abcdef0123456789abcdef01234567",
    latestSha: "fedcba9876543210fedcba9876543210fedcba98",
    behindBy: 1,
    commits: [{ sha: "fedcba9", message: "feat: newest", date: null }],
    checkedAt: "2026-11-04T10:30:00Z",
    branch: "web-parity",
    instructions: { systemd: "s", docker: "d" },
    ...over,
  };
}

describe("parseWebUpdateCheck · release mode", () => {
  it("a check without mode is commit mode", () => {
    const parsed = parseWebUpdateCheck(raw());
    expect(parsed.mode).toBe("commit");
    expect(parsed.latestVersion).toBeNull();
    expect(parsed.releaseUrl).toBeNull();
    expect(parsed.publishedAt).toBeNull();
  });

  it("a mode the console does not know is commit mode", () => {
    expect(parseWebUpdateCheck(raw({ mode: "nightly" })).mode).toBe("commit");
  });

  it("release fields are read", () => {
    const parsed = parseWebUpdateCheck(
      raw({
        mode: "release",
        latestSha: null,
        latestVersion: "4.0.35-web.2",
        releaseUrl:
          "https://github.com/alxwlw/cc-switch-web/releases/tag/v4.0.35-web.2",
        publishedAt: "2026-11-20T09:00:00Z",
        behindBy: 0,
        commits: [],
      }),
    );
    expect(parsed).toMatchObject({
      status: "behind",
      mode: "release",
      latestSha: null,
      latestVersion: "4.0.35-web.2",
      releaseUrl:
        "https://github.com/alxwlw/cc-switch-web/releases/tag/v4.0.35-web.2",
      publishedAt: "2026-11-20T09:00:00Z",
    });
  });

  it("a release that is behind without a version is rejected, as a commit one without a sha is", () => {
    expect(() =>
      parseWebUpdateCheck(raw({ mode: "release", latestSha: null })),
    ).toThrow("latestVersion");
    expect(() => parseWebUpdateCheck(raw({ latestSha: null }))).toThrow(
      "latestSha",
    );
  });
});
