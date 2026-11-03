/** `location.reload()` behind a seam: jsdom cannot spy on it, and tests must see the call. */
export function reloadPage(): void {
  window.location.reload();
}
