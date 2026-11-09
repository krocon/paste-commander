import { Injectable, computed, effect, inject } from '@angular/core';
import { Backend } from './backend';

const de = {
  'app.name': 'Paste Commander',

  'picker.search': 'Historie durchsuchen',
  'picker.searchPlaceholder': 'Suchen …',
  'picker.empty': 'Noch nichts kopiert.',
  'picker.emptyHint': 'Kopierte Texte erscheinen hier automatisch.',
  'picker.noMatches': 'Keine Treffer.',
  'picker.settings': 'Einstellungen öffnen',
  'picker.paused': 'Pausiert',

  'settings.nav': 'Bereiche',
  'settings.general': 'Allgemein',
  'settings.shortcuts': 'Tastenkürzel',
  'settings.privacy': 'Datenschutz',
  'settings.platform': 'Plattform',
  'settings.saved': 'Gespeichert',
  'settings.saveError': 'Speichern fehlgeschlagen',

  'settings.language': 'Sprache',
  'settings.language.system': 'Systemsprache',
  'settings.language.de': 'Deutsch',
  'settings.language.en': 'English',
  'settings.launchAtLogin': 'Beim Anmelden starten',
  'settings.historyLimit': 'Maximale Anzahl Einträge',
  'settings.historyLimitHint': 'Angeheftete Einträge zählen nicht mit ({min}–{max}).',
  'settings.persistHistory': 'Historie über Neustarts behalten',
  'settings.clearHistoryOnExit': 'Historie beim Beenden löschen',
  'settings.restoreClipboard': 'Ursprüngliche Zwischenablage nach dem Einfügen wiederherstellen',

  'settings.pickerShortcut': 'Historie öffnen',
  'settings.quickPaste': 'Eintrag {n} direkt einfügen',
  'settings.quickPasteHint': 'Fügt den Eintrag ohne Fenster in die aktive App ein.',
  'recorder.record': 'Aufnehmen',
  'recorder.recording': 'Tasten drücken … (Esc bricht ab)',
  'recorder.none': 'Nicht belegt',
  'recorder.clear': 'Entfernen',
  'recorder.needsModifier': 'Bitte mit mindestens einer Sondertaste kombinieren.',

  'settings.excludedApps': 'Ausgeschlossene Apps',
  'settings.excludedAppsHint': 'Kopien aus diesen Apps werden nie gespeichert. macOS: Bundle-ID, Windows: Prozessname (z. B. KeePassXC.exe), Linux: WM_CLASS.',
  'settings.excludedAppsAdd': 'Hinzufügen',
  'settings.excludedAppsPlaceholder': 'z. B. com.bitwarden.desktop',
  'settings.excludedAppsRemove': '{app} entfernen',
  'settings.privacyConcealed': 'Als vertraulich markierte Inhalte (Passwortmanager) werden immer ignoriert.',
  'settings.privacyLocal': 'Alle Daten bleiben lokal. Paste Commander stellt keine Netzwerkverbindungen her.',

  'platform.os': 'Betriebssystem',
  'platform.session': 'Sitzung',
  'platform.desktop': 'Desktop',
  'platform.clipboardWatch': 'Kopiervorgänge im Hintergrund erkennen',
  'platform.globalShortcuts': 'Globale Tastenkürzel',
  'platform.pasteSimulation': 'Direkt in die aktive App einfügen',
  'platform.supported': 'Unterstützt',
  'platform.limited': 'Eingeschränkt',
  'platform.unsupported': 'Nicht unterstützt',
  'platform.note.macAccessibility': 'Erfordert die Freigabe unter Systemeinstellungen › Datenschutz & Sicherheit › Bedienungshilfen.',
  'platform.note.windowsUipi': 'In Fenster von Programmen mit Administratorrechten kann nicht eingefügt werden.',
  'platform.note.gnomeNoDataControl': 'GNOME/Wayland erlaubt keinen Zugriff auf die Zwischenablage anderer Apps. Es werden nur Kopien erkannt, während Paste Commander im Vordergrund ist.',
  'platform.note.waylandDataControl': 'Erfordert einen Compositor mit ext-data-control-v1 oder wlr-data-control (KDE Plasma, Sway, Hyprland …).',
  'platform.note.portalShortcuts': 'Über das XDG-Portal „GlobalShortcuts“; der Desktop fragt beim ersten Start nach Erlaubnis.',
  'platform.note.waylandPaste': 'Über das XDG-Portal „RemoteDesktop“ bzw. virtuelle Tastatur; sonst wird nur kopiert und Strg+V bleibt bei dir.',
  'platform.note.unknownSession': 'Keine grafische Sitzung erkannt.',
} as const;

export type I18nKey = keyof typeof de;
type Catalog = Record<I18nKey, string>;

const en: Catalog = {
  'app.name': 'Paste Commander',

  'picker.search': 'Search history',
  'picker.searchPlaceholder': 'Search…',
  'picker.empty': 'Nothing copied yet.',
  'picker.emptyHint': 'Copied text shows up here automatically.',
  'picker.noMatches': 'No matches.',
  'picker.settings': 'Open settings',
  'picker.paused': 'Paused',

  'settings.nav': 'Sections',
  'settings.general': 'General',
  'settings.shortcuts': 'Shortcuts',
  'settings.privacy': 'Privacy',
  'settings.platform': 'Platform',
  'settings.saved': 'Saved',
  'settings.saveError': 'Saving failed',

  'settings.language': 'Language',
  'settings.language.system': 'System language',
  'settings.language.de': 'Deutsch',
  'settings.language.en': 'English',
  'settings.launchAtLogin': 'Launch at login',
  'settings.historyLimit': 'Maximum number of entries',
  'settings.historyLimitHint': 'Pinned entries do not count ({min}–{max}).',
  'settings.persistHistory': 'Keep history across restarts',
  'settings.clearHistoryOnExit': 'Clear history on quit',
  'settings.restoreClipboard': 'Restore the previous clipboard after pasting',

  'settings.pickerShortcut': 'Open history',
  'settings.quickPaste': 'Paste entry {n} directly',
  'settings.quickPasteHint': 'Pastes the entry into the active app without a window.',
  'recorder.record': 'Record',
  'recorder.recording': 'Press keys… (Esc cancels)',
  'recorder.none': 'Not set',
  'recorder.clear': 'Clear',
  'recorder.needsModifier': 'Please combine with at least one modifier key.',

  'settings.excludedApps': 'Excluded apps',
  'settings.excludedAppsHint': 'Copies from these apps are never stored. macOS: bundle ID, Windows: process name (e.g. KeePassXC.exe), Linux: WM_CLASS.',
  'settings.excludedAppsAdd': 'Add',
  'settings.excludedAppsPlaceholder': 'e.g. com.bitwarden.desktop',
  'settings.excludedAppsRemove': 'Remove {app}',
  'settings.privacyConcealed': 'Content marked as confidential (password managers) is always ignored.',
  'settings.privacyLocal': 'All data stays local. Paste Commander makes no network connections.',

  'platform.os': 'Operating system',
  'platform.session': 'Session',
  'platform.desktop': 'Desktop',
  'platform.clipboardWatch': 'Detect copies in the background',
  'platform.globalShortcuts': 'Global shortcuts',
  'platform.pasteSimulation': 'Paste directly into the active app',
  'platform.supported': 'Supported',
  'platform.limited': 'Limited',
  'platform.unsupported': 'Not supported',
  'platform.note.macAccessibility': 'Requires permission under System Settings › Privacy & Security › Accessibility.',
  'platform.note.windowsUipi': 'Cannot paste into windows of programs running as administrator.',
  'platform.note.gnomeNoDataControl': 'GNOME/Wayland does not let apps read other apps’ clipboard. Only copies made while Paste Commander is focused are detected.',
  'platform.note.waylandDataControl': 'Requires a compositor with ext-data-control-v1 or wlr-data-control (KDE Plasma, Sway, Hyprland…).',
  'platform.note.portalShortcuts': 'Via the XDG “GlobalShortcuts” portal; the desktop asks for permission on first use.',
  'platform.note.waylandPaste': 'Via the XDG “RemoteDesktop” portal or a virtual keyboard; otherwise the entry is copied and you press Ctrl+V.',
  'platform.note.unknownSession': 'No graphical session detected.',
};

const catalogs = { de, en } satisfies Record<'de' | 'en', Catalog>;

@Injectable({ providedIn: 'root' })
export class I18n {
  private readonly backend = inject(Backend);

  readonly lang = computed<'de' | 'en'>(() => {
    const setting = this.backend.settings()?.language ?? 'system';
    if (setting !== 'system') return setting;
    return navigator.language.toLowerCase().startsWith('de') ? 'de' : 'en';
  });

  constructor() {
    effect(() => document.documentElement.setAttribute('lang', this.lang()));
  }

  /** Reactive when called inside a template/computed: re-evaluates on language change. */
  t(key: I18nKey, params?: Record<string, string | number>): string {
    let text: string = catalogs[this.lang()][key] ?? key;
    if (params) {
      for (const [k, v] of Object.entries(params)) text = text.replaceAll(`{${k}}`, String(v));
    }
    return text;
  }
}
