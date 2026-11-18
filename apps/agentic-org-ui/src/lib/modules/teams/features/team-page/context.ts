// What a team page's tabs read from the page around them: which organization and team, and the team's name
// once it has loaded. Set by TeamPage; each tab's route renders its tab without props.
import { createContext } from 'svelte';

export interface TeamPageContext {
	readonly organizationId: string;
	readonly teamId: string;
	readonly teamName: string;
}

export const [teamPageContext, setTeamPageContext] = createContext<TeamPageContext>();
