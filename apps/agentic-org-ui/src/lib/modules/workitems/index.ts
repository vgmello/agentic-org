// Public entry point for the Work items module (work-items-ux, feature-requests-ux, costs-and-insights-ux §2/§4).
// Routes and other modules import from here rather than reaching into internal feature folders.
export { workItemsApi, type WorkItemsApi } from './api';
export { WORK_URL, taskHref, normalizeRef, isRef } from './present';
export type { TaskRow, TaskDetail, TaskType, WorkScope } from './types';
export { default as WorkQueueView } from './features/queue/WorkQueueView.svelte';
export { default as WorkDetailPage } from './features/detail/WorkDetailPage.svelte';
export { default as CaptureDialog } from './features/capture/CaptureDialog.svelte';
