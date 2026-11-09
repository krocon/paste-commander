import { DestroyRef, Injectable, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { PlatformInfo, Settings } from './models';

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Thin, signal-based facade over the Rust commands. Holds no logic of its own. */
@Injectable({ providedIn: 'root' })
export class Backend {
  private readonly _settings = signal<Settings | null>(null);
  private readonly _platform = signal<PlatformInfo | null>(null);
  private readonly _paused = signal(false);

  readonly settings = this._settings.asReadonly();
  readonly platform = this._platform.asReadonly();
  readonly paused = this._paused.asReadonly();

  constructor() {
    if (!isTauri) return;
    const destroyRef = inject(DestroyRef);
    const unlisteners = [
      listen<Settings>('settings-changed', (e) => this._settings.set(e.payload)),
      listen<boolean>('paused-changed', (e) => this._paused.set(e.payload)),
    ];
    destroyRef.onDestroy(() => unlisteners.forEach((p) => p.then((off) => off())));

    void this.reload();
  }

  async reload(): Promise<void> {
    const [settings, platform] = await Promise.all([
      invoke<Settings>('get_settings'),
      invoke<PlatformInfo>('get_platform_info'),
    ]);
    this._settings.set(settings);
    this._platform.set(platform);
  }

  /** Persists settings; the backend sanitizes and returns the stored version. */
  async updateSettings(settings: Settings): Promise<Settings> {
    const saved = await invoke<Settings>('update_settings', { settings });
    this._settings.set(saved);
    return saved;
  }

  hidePicker(): Promise<void> {
    return isTauri ? invoke('hide_picker') : Promise.resolve();
  }

  openSettings(): Promise<void> {
    return isTauri ? invoke('open_settings') : Promise.resolve();
  }
}
