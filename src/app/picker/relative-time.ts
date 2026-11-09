const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 24 * 3600],
  ['month', 30 * 24 * 3600],
  ['week', 7 * 24 * 3600],
  ['day', 24 * 3600],
  ['hour', 3600],
  ['minute', 60],
];

const formatters = new Map<string, Intl.RelativeTimeFormat>();

/** "vor 5 Min." / "5 min. ago"; under a minute → "jetzt" / "now". */
export function relativeTime(thenMs: number, nowMs: number, lang: 'de' | 'en'): string {
  let fmt = formatters.get(lang);
  if (!fmt) {
    fmt = new Intl.RelativeTimeFormat(lang, { numeric: 'auto', style: 'short' });
    formatters.set(lang, fmt);
  }
  const seconds = Math.round((thenMs - nowMs) / 1000);
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return fmt.format(Math.trunc(seconds / size), unit);
  }
  return fmt.format(0, 'second');
}
