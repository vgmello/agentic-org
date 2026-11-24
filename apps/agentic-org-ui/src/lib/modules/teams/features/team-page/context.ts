// What a team page's tabs read from the page around them: which organization and team, the loaded team detail,
// the route's load count, and optional toolbar controls / live open-work count reporting.
import { createContext, type Snippet } from 'svelte';
import type { TeamDetail } from '../../types';

export interface TeamPageContext {
	readonly organizationId: string;
	readonly teamId: string;
	readonly teamName: string;
	readonly detail: TeamDetail;
	/** The route's load count: a change means the organization changed, so the tabs re-read (`reloadOnChange`). */
	readonly loads: number;
	/** Optional view-specific controls rendered inside the team toolbar (e.g. Team Work). */
	setToolbarControls(controls: Snippet | undefined): void;
	/** Reports the real open work count once a tab reads team work so the Work tab badge stays reconciled. */
	setOpenWorkCount(count: number | undefined): void;
}

export const [teamPageContext, setTeamPageContext] = createContext<TeamPageContext>();
