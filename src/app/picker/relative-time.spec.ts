import { describe, expect, it } from 'vitest';
import { relativeTime } from './relative-time';

const now = Date.UTC(2026, 10, 9, 12, 0, 0);

describe('relativeTime', () => {
  it('says "now" below a minute', () => {
    expect(relativeTime(now - 20_000, now, 'en')).toBe('now');
    expect(relativeTime(now - 20_000, now, 'de')).toBe('jetzt');
  });

  it('uses the largest fitting unit', () => {
    expect(relativeTime(now - 5 * 60_000, now, 'en')).toMatch(/5 min/);
    expect(relativeTime(now - 3 * 3600_000, now, 'de')).toMatch(/3 Std/);
    expect(relativeTime(now - 24 * 3600_000, now, 'en')).toBe('yesterday');
  });
});
