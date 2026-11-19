/**
 * Closes a popover when a press lands outside it. Shared by the sidebar's
 * feed menu, the AI panel's history popover and the AI context chip list.
 *
 * `mousedown` in the capture phase, not `click`: a popover that closes on
 * `click` would first let the press focus whatever is underneath, and
 * capturing means a `stopPropagation` inside the popover can't swallow it.
 */
export function clickOutside(node: HTMLElement, onOutside: () => void) {
  function handle(event: MouseEvent) {
    if (!node.contains(event.target as Node)) onOutside();
  }
  document.addEventListener('mousedown', handle, true);
  return {
    destroy() {
      document.removeEventListener('mousedown', handle, true);
    },
  };
}
