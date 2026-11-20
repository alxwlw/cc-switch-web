import { describe, expect, it } from "vitest";
import { setWebTitle, webTitlePlugin } from "../../scripts/web-title";

describe("web build title", () => {
  it("replaces the upstream title and leaves the rest of the page alone", () => {
    const html =
      '<html><head><meta charset="UTF-8" /><title>Claude Code 供应商切换器</title></head><body><div id="root"></div></body></html>';
    expect(setWebTitle(html)).toBe(
      '<html><head><meta charset="UTF-8" /><title>cc-switch-web</title></head><body><div id="root"></div></body></html>',
    );
  });

  it("returns a page without a title unchanged", () => {
    expect(setWebTitle("<html><head></head></html>")).toBe(
      "<html><head></head></html>",
    );
  });

  it("the plugin retitles index.html and not the tray panel", () => {
    const hook = webTitlePlugin().transformIndexHtml as (
      html: string,
      ctx: { path: string },
    ) => string;
    const html = "<title>upstream</title>";
    expect(hook(html, { path: "/index.html" })).toBe(
      "<title>cc-switch-web</title>",
    );
    expect(hook(html, { path: "/tray-panel.html" })).toBe(html);
  });
});
