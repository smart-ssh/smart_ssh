// Issue #629: the WebView's built-in context menu offers "Reload", which
// wipes tabs and layout. Release builds suppress it everywhere except in
// editable text fields (cut/copy/paste) and the terminal (selection copy).

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false
  if (target.closest('input, textarea, [contenteditable=""], [contenteditable="true"]')) return true
  return target.closest('.xterm') !== null
}

/** Pure decision, testable with a build flag. */
export function shouldSuppressContextMenu(prod: boolean, event: Event): boolean {
  return prod && !event.defaultPrevented && !isEditableTarget(event.target)
}

export function installContextMenuSuppression(prod: boolean = import.meta.env.PROD): () => void {
  const handler = (e: Event) => {
    if (shouldSuppressContextMenu(prod, e)) e.preventDefault()
  }
  document.addEventListener('contextmenu', handler)
  return () => document.removeEventListener('contextmenu', handler)
}
