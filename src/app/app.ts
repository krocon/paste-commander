import { ChangeDetectionStrategy, Component } from '@angular/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauri } from './core/backend';
import { Picker } from './picker/picker';
import { SettingsPage } from './settings/settings';

type View = 'picker' | 'settings';

/** One bundle, two windows: the Tauri window label picks the view. */
function currentView(): View {
  if (isTauri) return getCurrentWindow().label === 'picker' ? 'picker' : 'settings';
  // Plain browser (`ng serve`) for UI work: ?view=picker
  return new URLSearchParams(location.search).get('view') === 'picker' ? 'picker' : 'settings';
}

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Picker, SettingsPage],
  template: `
    @if (view === 'picker') {
      <pc-picker />
    } @else {
      <pc-settings />
    }
  `,
})
export class App {
  protected readonly view = currentView();
}
