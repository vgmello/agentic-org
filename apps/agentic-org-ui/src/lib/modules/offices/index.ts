// The offices module's public API (offices-and-teams-ux §6): Organization settings › Offices.
export { officesApi, type OfficesApi } from './api';
export { layerOffices } from './layer';
export { default as OfficesPage } from './features/list/OfficesPage.svelte';
export { loadOfficeCounts, type Counts } from './features/list/counts';
