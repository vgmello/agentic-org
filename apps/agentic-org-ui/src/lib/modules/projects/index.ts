// The projects module's public entry (work-items-ux §3, offices-and-teams-ux §7.9). Routes and other modules import
// from here, never from its folders.
export { projectsApi, type ProjectsApi } from './api';
export { PROJECTS_URL, newProjectUrl, projectHref } from './present';
export { default as ProjectsList } from './features/list/ProjectsList.svelte';
export { default as ProjectFrame } from './features/frame/ProjectFrame.svelte';
export { default as ProjectOverview } from './features/overview/ProjectOverview.svelte';
export { default as ProjectWork } from './features/work/ProjectWork.svelte';
export { default as ProjectTeam } from './features/team/ProjectTeam.svelte';
export { default as ProjectActivity } from './features/activity/ProjectActivity.svelte';
export { default as ProjectConversations } from './features/conversations/ProjectConversations.svelte';
export { default as ProjectSettings } from './features/settings/ProjectSettings.svelte';
