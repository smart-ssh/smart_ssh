import { afterEach, describe, expect, it } from 'vitest'
import { installContextMenuSuppression } from './contextMenu.ts'

let off: (() => void) | null = null
afterEach(() => {
  off?.()
  off = null
  document.body.innerHTML = ''
})

function fire(el: Element): boolean {
  const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
  el.dispatchEvent(ev)
  return ev.defaultPrevented
}

describe('context menu suppression (issue #629)', () => {
  it('release: prevented outside editable elements, not in inputs or terminal', () => {
    off = installContextMenuSuppression(true)
    document.body.innerHTML = '<div id="d"></div><input id="i"><textarea id="t"></textarea><div class="xterm"><span id="x"></span></div>'
    expect(fire(document.getElementById('d')!)).toBe(true)
    expect(fire(document.getElementById('i')!)).toBe(false)
    expect(fire(document.getElementById('t')!)).toBe(false)
    expect(fire(document.getElementById('x')!)).toBe(false)
  })
  it('dev: WebView menu stays', () => {
    off = installContextMenuSuppression(false)
    document.body.innerHTML = '<div id="d"></div>'
    expect(fire(document.getElementById('d')!)).toBe(false)
  })
})
