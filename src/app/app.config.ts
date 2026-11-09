import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';

// Zoneless change detection is the default since Angular 21.
export const appConfig: ApplicationConfig = {
  providers: [provideBrowserGlobalErrorListeners()],
};
