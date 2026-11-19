// The preferences module's public entry (app-shell-ux, Your preferences). Routes and other modules import from here,
// never from its files: the account's time zone, the time formats that use it, and the preference sections.
export { preferencesApi, type PreferencesApi } from './api';
export { accountZone } from './account-zone.svelte';
export * from './time';
export { default as ZoneNotice } from './ZoneNotice.svelte';
export { default as KeyboardSection } from './features/keyboard/KeyboardSection.svelte';
export { default as TimeZoneSection } from './features/time-zone/TimeZoneSection.svelte';
