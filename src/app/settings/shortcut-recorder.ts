import { ChangeDetectionStrategy, Component, computed, inject, input, output, signal } from '@angular/core';
import { acceleratorFromEvent, formatAccelerator } from '../core/accelerator';
import { Backend } from '../core/backend';
import { I18n } from '../core/i18n';

@Component({
  selector: 'pc-shortcut-recorder',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button
      type="button"
      class="field"
      [class.recording]="recording()"
      [attr.aria-label]="label() + ': ' + display()"
      [attr.aria-pressed]="recording()"
      (click)="toggle()"
      (keydown)="onKey($event)"
      (blur)="stop()">
      @if (recording()) {
        <span class="hint">{{ i18n.t('recorder.recording') }}</span>
      } @else if (value()) {
        <kbd>{{ display() }}</kbd>
      } @else {
        <span class="hint">{{ i18n.t('recorder.none') }}</span>
      }
    </button>
    @if (value() && !recording()) {
      <button type="button" class="clear" (click)="valueChange.emit(null)"
              [attr.aria-label]="i18n.t('recorder.clear') + ': ' + label()">×</button>
    }
    @if (error() || status()) {
      <span class="error" role="alert">{{ error() || status() }}</span>
    }
  `,
  styles: `
    :host { display: inline-flex; align-items: center; gap: var(--pc-space-2); flex-wrap: wrap; }
    .field {
      min-width: 180px; height: 30px; padding: 0 var(--pc-space-3);
      border: 1px solid var(--pc-border); border-radius: var(--pc-radius-sm);
      background: var(--pc-bg); text-align: left; cursor: pointer;
      transition: border-color var(--pc-transition), box-shadow var(--pc-transition);
    }
    .field.recording { border-color: var(--pc-accent); box-shadow: 0 0 0 3px var(--pc-accent-soft); }
    kbd { font-family: var(--pc-font-mono); font-size: var(--pc-font-size-sm); letter-spacing: 0.02em; }
    .hint { color: var(--pc-fg-muted); }
    .clear {
      width: 24px; height: 24px; border: 0; border-radius: 50%;
      background: var(--pc-bg-subtle); color: var(--pc-fg-muted); cursor: pointer; line-height: 1;
    }
    .clear:hover { color: var(--pc-fg); }
    .error { width: 100%; color: var(--pc-error); font-size: var(--pc-font-size-sm); }
  `,
})
export class ShortcutRecorder {
  protected readonly i18n = inject(I18n);
  private readonly backend = inject(Backend);

  readonly value = input<string | null>(null);
  readonly label = input.required<string>();
  /** Registration error reported by the backend for this shortcut. */
  readonly status = input<string | null>(null);
  readonly valueChange = output<string | null>();

  protected readonly recording = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly display = computed(() => {
    const v = this.value();
    return v ? formatAccelerator(v, this.i18n.lang()) : this.i18n.t('recorder.none');
  });

  protected toggle(): void {
    this.error.set(null);
    if (this.recording()) this.stop();
    else {
      this.recording.set(true);
      // Our own global shortcuts must not fire while recording.
      void this.backend.suspendShortcuts(true);
    }
  }

  protected stop(): void {
    if (!this.recording()) return;
    this.recording.set(false);
    void this.backend.suspendShortcuts(false);
  }

  protected onKey(e: KeyboardEvent): void {
    if (!this.recording()) return;
    e.preventDefault();
    e.stopPropagation();
    const result = acceleratorFromEvent(e);
    switch (result.kind) {
      case 'pending':
        return;
      case 'needsModifier':
        this.error.set(this.i18n.t('recorder.needsModifier'));
        return;
      case 'cancel':
        break;
      case 'clear':
        this.valueChange.emit(null);
        break;
      case 'done':
        this.valueChange.emit(result.accelerator);
        break;
    }
    this.error.set(null);
    this.stop();
  }
}
