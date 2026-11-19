// The processes module's public API (processes-ux; development-processes design): Organization settings › Processes,
// the document editor, development processes, and the Processes tab other pages embed (a team's, an agent's).
export { default as ProcessesPage } from './features/place-view/ProcessesPage.svelte';
export { default as DocumentEditor } from './features/editor/DocumentEditor.svelte';
export { editorMode } from './features/editor/load';
export { default as DevCatalogue } from './features/dev-catalogue/DevCatalogue.svelte';
export { default as DevProcessPage } from './features/dev-process/DevProcessPage.svelte';
export { default as PlaceProcesses } from './features/embedded/PlaceProcesses.svelte';
export { readPlace, type PlaceRead } from './features/place-view/load';
export { PROCESSES_URL, DEV_PROCESSES_URL, listHref, devProcessHref } from './present';
export type { Place } from './types';
