import type { Plugin } from "vite";

export const WEB_TITLE = "cc-switch-web";

/** Replaces the first `<title>` of an HTML document; the document is returned as is without one. */
export function setWebTitle(html: string): string {
  return html.replace(/<title>[^<]*<\/title>/, `<title>${WEB_TITLE}</title>`);
}

/**
 * `src/index.html` is the upstream desktop file with its own title; the fork's web build
 * names itself instead of editing that file.
 */
export function webTitlePlugin(): Plugin {
  return {
    name: "cc-switch-web-title",
    // The tray panel is a second entry of the same build and keeps its own title.
    transformIndexHtml: (html, ctx) =>
      ctx.path === "/index.html" ? setWebTitle(html) : html,
  };
}
