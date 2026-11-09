import { ChangeDetectionStrategy, Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { Backend } from '../core/backend';
import { I18n, I18nKey } from '../core/i18n';
import {
  Capability, HISTORY_LIMIT_MAX, HISTORY_LIMIT_MIN, Language, Settings, ShortcutAction,
} from '../core/models';
import { ShortcutRecorder } from './shortcut-recorder';

type Section = 'general' | 'shortcuts' | 'privacy' | 'platform';

@Component({
  selector: 'pc-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ShortcutRecorder],
  templateUrl: './settings.html',
  host: { '(window:focus)': 'backend.refreshPlatform()' },
  styleUrl: './settings.css',
})
export class SettingsPage {
  protected readonly backend = inject(Backend);
  protected readonly i18n = inject(I18n);

  protected readonly sections: { id: Section; key: I18nKey }[] = [
    { id: 'general', key: 'settings.general' },
    { id: 'shortcuts', key: 'settings.shortcuts' },
    { id: 'privacy', key: 'settings.privacy' },
    { id: 'platform', key: 'settings.platform' },
  ];
  protected readonly section = signal<Section>('general');
  protected readonly status = signal<'idle' | 'saved' | 'error'>('idle');
  protected readonly newExcludedApp = signal('');

  protected readonly s = this.backend.settings;
  protected readonly limits = { min: HISTORY_LIMIT_MIN, max: HISTORY_LIMIT_MAX };
  protected readonly languages: Language[] = ['system', 'de', 'en'];
  /** Quick-paste slots map to history positions 2..9. */
  protected readonly quickSlots = computed(() =>
    (this.s()?.shortcuts.quickPaste ?? []).map((value, i) => ({ value, position: i + 2 })),
  );

  protected readonly capabilities = computed(() => {
    const p = this.backend.platform();
    if (!p) return [];
    const rows: { key: I18nKey; cap: Capability }[] = [
      { key: 'platform.clipboardWatch', cap: p.clipboardWatch },
      { key: 'platform.globalShortcuts', cap: p.globalShortcuts },
      { key: 'platform.pasteSimulation', cap: p.pasteSimulation },
    ];
    return rows;
  });

  private statusTimer?: ReturnType<typeof setTimeout>;

  constructor() {
    // First thing a macOS user without Accessibility permission should see.
    let decided = false;
    effect(() => {
      const p = this.backend.platform();
      if (!p || decided) return;
      decided = true;
      if (p.accessibilityGranted === false) untracked(() => this.section.set('platform'));
    });
  }

  protected shortcutError(action: ShortcutAction): string | null {
    const match = this.backend
      .shortcutStatus()
      .find((s) => s.action.kind === action.kind && (action.kind === 'picker' || (s.action.kind === 'quickPaste' && s.action.position === action.position)));
    return match?.error ?? null;
  }

  protected async patch(change: (draft: Settings) => void): Promise<void> {
    const current = this.s();
    if (!current) return;
    const draft = structuredClone(current);
    change(draft);
    try {
      await this.backend.updateSettings(draft);
      this.flash('saved');
    } catch (e) {
      console.error(e);
      this.flash('error');
      await this.backend.reload();
    }
  }

  protected setHistoryLimit(raw: string): void {
    const n = Number.parseInt(raw, 10);
    if (Number.isFinite(n)) void this.patch((d) => (d.historyLimit = n));
  }

  protected setQuickPaste(index: number, value: string | null): void {
    void this.patch((d) => (d.shortcuts.quickPaste[index] = value));
  }

  protected addExcludedApp(): void {
    const app = this.newExcludedApp().trim();
    if (!app) return;
    void this.patch((d) => d.excludedApps.push(app)).then(() => this.newExcludedApp.set(''));
  }

  protected removeExcludedApp(app: string): void {
    void this.patch((d) => (d.excludedApps = d.excludedApps.filter((a) => a !== app)));
  }

  protected supportKey(cap: Capability): I18nKey {
    return `platform.${cap.support}`;
  }

  protected noteKey(cap: Capability): I18nKey | null {
    return cap.note as I18nKey | null;
  }

  private flash(status: 'saved' | 'error'): void {
    this.status.set(status);
    clearTimeout(this.statusTimer);
    this.statusTimer = setTimeout(() => this.status.set('idle'), 1500);
  }
}
