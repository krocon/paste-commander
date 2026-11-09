import { describe, expect, it } from 'vitest';
import { acceleratorFromEvent, formatAccelerator } from './accelerator';

const ev = (code: string, mods: Partial<Record<'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey', boolean>> = {}) => ({
  code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods,
});

describe('acceleratorFromEvent', () => {
  it('builds accelerators from physical keys', () => {
    expect(acceleratorFromEvent(ev('KeyV', { ctrlKey: true, altKey: true }))).toEqual({ kind: 'done', accelerator: 'Ctrl+Alt+V' });
    expect(acceleratorFromEvent(ev('Digit2', { ctrlKey: true, altKey: true, shiftKey: true }))).toEqual({ kind: 'done', accelerator: 'Ctrl+Alt+Shift+2' });
  });

  it('waits while only modifiers are held', () => {
    expect(acceleratorFromEvent(ev('ShiftLeft', { shiftKey: true }))).toEqual({ kind: 'pending' });
  });

  it('requires a real modifier except for function keys', () => {
    expect(acceleratorFromEvent(ev('KeyA', { shiftKey: true }))).toEqual({ kind: 'needsModifier' });
    expect(acceleratorFromEvent(ev('F9'))).toEqual({ kind: 'done', accelerator: 'F9' });
  });

  it('handles cancel and clear', () => {
    expect(acceleratorFromEvent(ev('Escape'))).toEqual({ kind: 'cancel' });
    expect(acceleratorFromEvent(ev('Backspace'))).toEqual({ kind: 'clear' });
  });
});

describe('formatAccelerator', () => {
  it('uses symbols on macOS in canonical order', () => {
    expect(formatAccelerator('Cmd+Shift+V', 'en', true)).toBe('⇧⌘V');
    expect(formatAccelerator('Ctrl+Alt+2', 'de', true)).toBe('⌃⌥2');
  });

  it('localizes Ctrl elsewhere', () => {
    expect(formatAccelerator('Ctrl+Alt+V', 'de', false)).toBe('Strg+Alt+V');
    expect(formatAccelerator('Ctrl+Alt+V', 'en', false)).toBe('Ctrl+Alt+V');
  });
});
