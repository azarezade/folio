// Keyboard activation for `role="button"` elements. Tiles whose text is
// truncated are not <button>s: WebKit (the Mac app's engine) wraps a
// button's contents in an inner box that grows to fit, so `text-overflow:
// ellipsis` never applies and long names spill over their neighbours.
export function activate(e: KeyboardEvent) {
  if ((e.key === 'Enter' || e.key === ' ') && e.target === e.currentTarget) {
    e.preventDefault();
    (e.currentTarget as HTMLElement).click();
  }
}
