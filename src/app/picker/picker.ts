import {
  ChangeDetectionStrategy, Component, DestroyRef, ElementRef, afterNextRender, computed, effect,
  inject, signal, viewChild,
} from '@angular/core';
import { isMac } from '../core/accelerator';
import { Backend } from '../core/backend';
import { I18n } from '../core/i18n';
import type { EntrySummary } from '../core/models';
import { relativeTime } from './relative-time';

/** Rows shown at most; search goes to Rust over the full text anyway. */
const MAX_ROWS = 500;

@Component({
  selector: 'pc-picker',
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './picker.html',
  styleUrl: './picker.css',
  host: { '(document:keydown)': 'onKey($event)' },
})
export class Picker {
  protected readonly backend = inject(Backend);
  protected readonly i18n = inject(I18n);
  protected readonly mod = isMac ? '⌘' : 'Ctrl';
  protected readonly pasteKeys = isMac ? '⌘V' : 'Ctrl+V';

  protected readonly query = signal('');
  protected readonly entries = signal<EntrySummary[]>([]);
  protected readonly selected = signal(0);
  /** Shown instead of hiding when we could only copy. */
  protected readonly copiedHint = signal<null | { accessibility: boolean }>(null);
  protected readonly now = signal(Date.now());

  protected readonly rows = computed(() => this.entries().slice(0, MAX_ROWS));
  protected readonly activeId = computed(() => {
    const e = this.rows()[this.selected()];
    return e ? `pc-entry-${e.id}` : null;
  });

  private readonly search = viewChild.required<ElementRef<HTMLInputElement>>('search');
  private readonly list = viewChild.required<ElementRef<HTMLElement>>('list');
  private loadSeq = 0;
  private hintTimer?: ReturnType<typeof setTimeout>;

  constructor() {
    document.documentElement.classList.add('pc-picker');
    const destroyRef = inject(DestroyRef);

    const offs = [
      this.backend.onPickerShown(() => this.onShown()),
      this.backend.onHistoryChanged(() => void this.load()),
    ];
    destroyRef.onDestroy(() => offs.forEach((p) => p.then((off) => off())));

    // Re-query whenever the search text changes; stale responses are dropped.
    effect(() => {
      this.query();
      void this.load();
    });

    // Keep the selected row visible.
    effect(() => {
      const id = this.activeId();
      if (id) queueMicrotask(() => document.getElementById(id)?.scrollIntoView({ block: 'nearest' }));
    });

    afterNextRender(() => this.focusSearch());
  }

  protected rel(ms: number): string {
    return relativeTime(ms, this.now(), this.i18n.lang());
  }

  private onShown(): void {
    clearTimeout(this.hintTimer);
    this.copiedHint.set(null);
    this.now.set(Date.now());
    this.selected.set(0);
    if (this.query()) this.query.set(''); // triggers load via effect
    else void this.load();
    this.list().nativeElement.scrollTop = 0;
    this.focusSearch();
  }

  private async load(): Promise<void> {
    const seq = ++this.loadSeq;
    const entries = await this.backend.getHistory(this.query());
    if (seq !== this.loadSeq) return;
    this.entries.set(entries);
    this.selected.update((i) => Math.min(i, Math.max(0, entries.length - 1)));
  }

  protected focusSearch(): void {
    const input = this.search().nativeElement;
    input.focus();
    input.select();
  }

  protected onQuery(value: string): void {
    this.selected.set(0);
    this.query.set(value);
  }

  protected async paste(index: number): Promise<void> {
    const entry = this.rows()[index];
    if (!entry) return;
    const outcome = await this.backend.pasteEntry(entry.id);
    if (outcome.kind === 'copied') {
      // Keep the window up briefly so the user learns why nothing was pasted.
      this.copiedHint.set({ accessibility: outcome.reason === 'noAccessibility' });
      clearTimeout(this.hintTimer);
      if (outcome.reason !== 'noAccessibility') {
        this.hintTimer = setTimeout(() => void this.backend.hidePicker(), 1400);
      }
    }
  }

  protected async togglePin(entry: EntrySummary): Promise<void> {
    await this.backend.setPinned(entry.id, !entry.pinned);
  }

  protected async remove(entry: EntrySummary): Promise<void> {
    await this.backend.deleteEntry(entry.id);
  }

  protected grantAccessibility(): void {
    void this.backend.openAccessibilitySettings();
    void this.backend.hidePicker();
  }

  protected onKey(e: KeyboardEvent): void {
    const count = this.rows().length;
    const primary = isMac ? e.metaKey : e.ctrlKey;

    if (e.key === 'Escape') {
      e.preventDefault();
      void this.backend.hidePicker();
      return;
    }
    if (primary && /^Digit[1-9]$/.test(e.code)) {
      e.preventDefault();
      void this.paste(Number(e.code.slice(5)) - 1);
      return;
    }
    if (primary && e.code === 'KeyP') {
      e.preventDefault();
      const entry = this.rows()[this.selected()];
      if (entry) void this.togglePin(entry);
      return;
    }
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        if (count) this.selected.update((i) => (i + 1) % count);
        break;
      case 'ArrowUp':
        e.preventDefault();
        if (count) this.selected.update((i) => (i - 1 + count) % count);
        break;
      case 'PageDown':
        e.preventDefault();
        this.selected.update((i) => Math.min(count - 1, i + 8));
        break;
      case 'PageUp':
        e.preventDefault();
        this.selected.update((i) => Math.max(0, i - 8));
        break;
      case 'Enter':
        // ⇧⏎ = plain text; entries are plain text already, so both paste.
        e.preventDefault();
        void this.paste(this.selected());
        break;
      case 'Delete': {
        e.preventDefault();
        const entry = this.rows()[this.selected()];
        if (entry) void this.remove(entry);
        break;
      }
    }
  }
}
