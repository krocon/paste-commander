import {
  ChangeDetectionStrategy, Component, ElementRef, afterNextRender, inject, signal, viewChild,
} from '@angular/core';
import { Backend } from '../core/backend';
import { I18n } from '../core/i18n';

/**
 * Picker shell. Phase 4 adds the history list, filtering and keyboard selection;
 * this phase only wires search focus, Esc-to-hide and the empty state.
 */
@Component({
  selector: 'pc-picker',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown)': 'onKey($event)', '(window:focus)': 'focusSearch()' },
  template: `
    <section class="panel" [attr.aria-label]="i18n.t('app.name')">
      <header>
        <input #search type="search" class="search" role="combobox" aria-expanded="true"
               aria-controls="pc-history" aria-autocomplete="list"
               [attr.aria-label]="i18n.t('picker.search')"
               [placeholder]="i18n.t('picker.searchPlaceholder')"
               [value]="query()" (input)="query.set($any($event.target).value)"
               spellcheck="false" autocomplete="off">
        @if (backend.paused()) {
          <span class="paused">{{ i18n.t('picker.paused') }}</span>
        }
        <button type="button" class="icon" (click)="backend.openSettings()" [attr.aria-label]="i18n.t('picker.settings')">⚙︎</button>
      </header>
      <ul id="pc-history" role="listbox" class="list" [attr.aria-label]="i18n.t('picker.search')">
        <li class="empty" role="presentation">
          <strong>{{ query() ? i18n.t('picker.noMatches') : i18n.t('picker.empty') }}</strong>
          @if (!query()) {
            <span>{{ i18n.t('picker.emptyHint') }}</span>
          }
        </li>
      </ul>
    </section>
  `,
  styles: `
    :host { display: block; height: 100vh; padding: 10px; }
    .panel {
      display: flex; flex-direction: column; height: 100%; overflow: hidden;
      border-radius: var(--pc-radius-lg); background: var(--pc-bg-elevated);
      backdrop-filter: blur(24px) saturate(160%); box-shadow: var(--pc-shadow);
    }
    header {
      display: flex; align-items: center; gap: var(--pc-space-2);
      padding: var(--pc-space-3); border-bottom: 1px solid var(--pc-border);
    }
    .search {
      flex: 1; height: 32px; padding: 0 var(--pc-space-2); border: 0; outline: 0;
      background: transparent; font-size: var(--pc-font-size-lg);
    }
    .search::-webkit-search-cancel-button { display: none; }
    .paused {
      padding: 2px 8px; border-radius: 999px; font-size: var(--pc-font-size-sm);
      color: var(--pc-warn); background: color-mix(in srgb, var(--pc-warn) 14%, transparent);
    }
    .icon { border: 0; background: none; color: var(--pc-fg-muted); font-size: 16px; cursor: pointer; }
    .icon:hover { color: var(--pc-fg); }
    .list { flex: 1; margin: 0; padding: var(--pc-space-2); list-style: none; overflow-y: auto; }
    .empty {
      display: grid; gap: var(--pc-space-1); place-content: center; height: 100%;
      text-align: center; color: var(--pc-fg-muted);
    }
    .empty strong { color: var(--pc-fg); font-weight: 500; }
  `,
})
export class Picker {
  protected readonly backend = inject(Backend);
  protected readonly i18n = inject(I18n);
  protected readonly query = signal('');
  private readonly search = viewChild.required<ElementRef<HTMLInputElement>>('search');

  constructor() {
    document.documentElement.classList.add('pc-picker');
    afterNextRender(() => this.focusSearch());
  }

  protected focusSearch(): void {
    this.search().nativeElement.focus();
    this.search().nativeElement.select();
  }

  protected onKey(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      this.query.set('');
      void this.backend.hidePicker();
    }
  }
}
