// The projects module's public entry (work-items-ux §3, offices-and-teams-ux §7.9). Routes and other modules import
// from here, never from its folders.
export { projectsApi, type ProjectsApi } from './api';
export type { ProjectRow } from './types';
export { PROJECTS_URL, newProjectUrl, projectHref } from './present';
export { projectFrame, type ProjectFrameContext } from './features/frame/context';
