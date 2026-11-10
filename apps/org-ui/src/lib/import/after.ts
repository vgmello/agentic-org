import { markArrived } from './arrived.svelte';
import type { BringInResult } from './bring-in';

/** What to say after "Bring in" closed: the agents it made are tagged on the chart for a while, and the page toasts what happened. */
export function announceBroughtIn(done: BringInResult | undefined, madeIds: string[]): string | undefined {
	if (!done) return undefined;
	markArrived(madeIds);
	const { agents, teams, projects, paused } = done;
	if (agents + teams + projects === 0) return undefined;
	const count = (n: number, one: string) => `${n} ${n === 1 ? one : `${one}s`}`;
	return `Brought in ${count(agents, 'agent')}, ${count(teams, 'team')} and ${count(projects, 'project')}.${paused > 0 ? ` ${count(paused, 'agent')} paused.` : ''}`;
}

/** The ids of the members whose messaging names these are. */
export const idsOf = (members: { id: string; agent?: { messaging_name: string } }[], handles: string[]): string[] => {
	const wanted = new Set(handles.map((h) => h.toLowerCase()));
	return members.filter((m) => m.agent && wanted.has(m.agent.messaging_name.toLowerCase())).map((m) => m.id);
};
