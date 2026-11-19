// The words of the office flows that depend on counts (offices-and-teams-ux §6.2, §6.4). Pure, so the copy is testable.

export const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "Lumen Labs'" and "Acme's" (§6.1: "Where Lumen Labs' agents run"). */
export const possessive = (name: string) => (/s$/i.test(name) ? `${name}'` : `${name}'s`);

/** "Kai, Juno and Nadia". */
export function nameList(names: string[]): string {
	if (names.length <= 1) return names[0] ?? '';
	return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

export interface RemoveFacts {
	office: string;
	organization: string;
	/** Undefined while the agents are counted, or when they couldn't be: the cost is said as unknown, never as none. */
	agents: number | undefined;
	tasks: number;
	offline: boolean;
}

/** The Remove office confirmation (§6.4): the office, its agents (which become unplaced), their tasks and the offline case. */
export function removeOfficeText(f: RemoveFacts): string {
	const parts = [`Remove ${f.office} from ${f.organization}? It can't sync again.`];
	if (f.agents === undefined)
		parts.push(
			"Its agents couldn't be counted: any agent placed here stops running there and becomes unplaced, and HQ takes back their tasks."
		);
	else if (f.agents > 0) {
		parts.push(
			`${f.agents === 1 ? 'Its agent stops' : `Its ${f.agents} agents stop`} running there and ${f.agents === 1 ? 'becomes' : 'become'} unplaced: you choose a new office for ${f.agents === 1 ? 'it' : 'each'}, or offboard ${f.agents === 1 ? 'it' : 'them'}.`
		);
		if (f.tasks > 0)
			parts.push(
				`HQ takes back ${f.agents === 1 ? 'its' : 'their'} ${plural(f.tasks, 'task', 'tasks')}.`
			);
	}
	if (f.offline)
		parts.push(
			`If ${f.office} is offline, it keeps its copy until it next connects, then deletes it.`
		);
	parts.push('To add it again, send a new invitation.');
	return parts.join(' ');
}

/** What an Add office waiting for running agents says (§6.2). */
export function waitingText(names: string[]): string {
	return `Waiting for ${plural(names.length, 'agent', 'agents')} to stop: ${nameList(names)}. Their active work and process cleanup must finish first`;
}

/**
 * What a polite live region says when an office's status changes (§6.1: "Office 2 is online", "Office 2 stopped: it
 * stopped unexpectedly"). Checking is not a change worth saying; `undefined` then.
 */
export function statusChangeText(name: string, status: string, why?: string): string | undefined {
	switch (status) {
		case 'active':
			return `${name} is online`;
		case 'starting':
			return `${name} is starting`;
		case 'offline':
			return why ? `${name} stopped: ${lower(why)}` : `${name} is offline`;
		case 'unavailable':
			return `${name} stopped${why ? `: ${lower(why)}` : ''}`;
		case 'update_needed':
			return `${name} needs an update`;
		case 'unreachable':
			return `${name} can't be reached`;
		case 'checking':
			return undefined;
		default:
			return `${name}'s status is unknown`;
	}
}
const lower = (text: string) => text.charAt(0).toLowerCase() + text.slice(1);
