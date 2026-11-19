// The access module's public entry (server-installation-and-access-ux §4a; offices-and-teams-ux §7.4). Routes import from here.
export { accessApi, type AccessApi } from './api';
export type * from './types';
export { default as AccessPage } from './features/overview/AccessPage.svelte';
export { groupPeople, stateWord, MANAGERS_ONLY } from './present';
