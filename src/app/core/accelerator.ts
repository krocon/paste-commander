// Converts keyboard events into the accelerator syntax of
// tauri-plugin-global-shortcut ("Cmd+Shift+V", "Ctrl+Alt+Shift+2") and back
// into a human-readable label.

export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

const MODIFIER_CODES = new Set([
  'ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight',
  'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight', 'OSLeft', 'OSRight',
]);

/** Uses `code` (physical key) so layouts like QWERTZ or AltGr don't change the binding. */
function keyFromCode(code: string): string | null {
  let m: RegExpMatchArray | null;
  if ((m = code.match(/^Key([A-Z])$/))) return m[1];
  if ((m = code.match(/^Digit(\d)$/))) return m[1];
  if (/^Numpad\d$/.test(code)) return code;
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code;
  const named: Record<string, string> = {
    Space: 'Space', Enter: 'Enter', Tab: 'Tab', Backspace: 'Backspace', Delete: 'Delete',
    Insert: 'Insert', Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown',
    ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right',
    Backquote: 'Backquote', Minus: 'Minus', Equal: 'Equal', BracketLeft: 'BracketLeft',
    BracketRight: 'BracketRight', Backslash: 'Backslash', Semicolon: 'Semicolon',
    Quote: 'Quote', Comma: 'Comma', Period: 'Period', Slash: 'Slash',
  };
  return named[code] ?? null;
}

export type RecordResult =
  | { kind: 'pending' }
  | { kind: 'cancel' }
  | { kind: 'clear' }
  | { kind: 'needsModifier' }
  | { kind: 'done'; accelerator: string };

export function acceleratorFromEvent(e: Pick<KeyboardEvent, 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>): RecordResult {
  const hasModifier = e.metaKey || e.ctrlKey || e.altKey;
  if (!hasModifier && !e.shiftKey) {
    if (e.code === 'Escape') return { kind: 'cancel' };
    if (e.code === 'Backspace' || e.code === 'Delete') return { kind: 'clear' };
  }
  if (MODIFIER_CODES.has(e.code)) return { kind: 'pending' };

  const key = keyFromCode(e.code);
  if (!key) return { kind: 'pending' };
  // Shift alone is not enough for a global shortcut (would swallow capitals).
  if (!hasModifier && !/^F\d+$/.test(key)) return { kind: 'needsModifier' };

  const parts: string[] = [];
  if (e.metaKey) parts.push(isMac ? 'Cmd' : 'Super');
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  parts.push(key);
  return { kind: 'done', accelerator: parts.join('+') };
}

const MAC_SYMBOLS: Record<string, string> = { Cmd: '⌘', Ctrl: '⌃', Alt: '⌥', Shift: '⇧' };
const MAC_ORDER = ['Ctrl', 'Alt', 'Shift', 'Cmd'];

/** "Cmd+Shift+V" → "⇧⌘V" on macOS, "Ctrl+Alt+V" → "Strg+Alt+V" in German elsewhere. */
export function formatAccelerator(acc: string, lang: 'de' | 'en', mac = isMac): string {
  const parts = acc.split('+');
  const key = parts.pop() ?? '';
  if (mac) {
    const mods = MAC_ORDER.filter((m) => parts.includes(m)).map((m) => MAC_SYMBOLS[m]);
    return mods.join('') + key;
  }
  const label = (p: string) => (p === 'Ctrl' && lang === 'de' ? 'Strg' : p === 'Super' ? 'Win' : p);
  return [...parts.map(label), key].join('+');
}
