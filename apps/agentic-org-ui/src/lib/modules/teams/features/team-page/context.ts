// What a team page's tabs read from the page around them: which organization and team, the loaded team detail,
// the route's load count, the frame-owned work resource (shared by Overview, Work, and the Work tab badge),
// and optional toolbar controls.
import { createContext, type Snippet } from 'svelte';
import type { Resource } from '$lib/components/resource.svelte';
import type { WorkRow } from '$lib/modules/members';
import type { TeamDetail } from '../../types';

export interface TeamPageContext {
	readonly organizationId: string;
	readonly teamId: string;
	readonly teamName: string;
	readonly detail: TeamDetail;
	/** The route's load count: a change means the organization changed, so the tabs re-read (`reloadOnChange`). */
	readonly loads: number;
	/** Shared team work resource owned by TeamPage so the Work badge and tabs stay synchronized. */
	readonly work: Resource<WorkRow[]>;
	/** Optional view-specific controls rendered inside the team toolbar (e.g. Team Work). */
	setToolbarControls(controls: Snippet | undefined): void;
}

export const [teamPageContext, setTeamPageContext] = createContext<TeamPageContext>();
