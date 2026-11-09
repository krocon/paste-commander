// Mirrors the serde types in src-tauri/src/{settings,platform}.rs.

export type Language = 'system' | 'de' | 'en';

export interface Shortcuts {
  picker: string | null;
  /** Index 0 pastes history entry 2, index 7 entry 9. */
  quickPaste: (string | null)[];
}

export interface Settings {
  language: Language;
  historyLimit: number;
  persistHistory: boolean;
  clearHistoryOnExit: boolean;
  restoreClipboard: boolean;
  launchAtLogin: boolean;
  excludedApps: string[];
  shortcuts: Shortcuts;
}

export const HISTORY_LIMIT_MIN = 10;
export const HISTORY_LIMIT_MAX = 500;

export type Os = 'macos' | 'windows' | 'linux' | 'other';
export type DisplayServer = 'native' | 'x11' | 'wayland' | 'unknown';
export type Support = 'supported' | 'limited' | 'unsupported';

export interface Capability {
  support: Support;
  note: string | null;
}

export interface PlatformInfo {
  os: Os;
  arch: string;
  displayServer: DisplayServer;
  desktop: string | null;
  clipboardWatch: Capability;
  globalShortcuts: Capability;
  pasteSimulation: Capability;
  /** Watcher actually running, e.g. `wayland-data-control`. */
  clipboardBackend: string | null;
}

export type EntryKind = 'text';

/** Mirrors `history::EntrySummary`: a bounded preview, never the full text. */
export interface EntrySummary {
  id: number;
  kind: EntryKind;
  preview: string;
  /** Full length in characters. */
  chars: number;
  sourceApp: string | null;
  /** Unix milliseconds. */
  createdAt: number;
  copiedAt: number;
  pinned: boolean;
}
