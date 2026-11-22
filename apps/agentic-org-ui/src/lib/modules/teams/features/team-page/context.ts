// What a team page's tabs read from the page around them: which organization and team, and the team's name
// once it has loaded, and the route's load count. Set by TeamPage; each tab's route renders its tab without props.
import { createContext } from 'svelte';

export interface TeamPageContext {
	readonly organizationId: string;
	readonly teamId: string;
	readonly teamName: string;
	/** The route's load count: a change means the organization changed, so the tabs re-read (`reloadOnChange`). */
	readonly loads: number;
}

export const [teamPageContext, setTeamPageContext] = createContext<TeamPageContext>();
