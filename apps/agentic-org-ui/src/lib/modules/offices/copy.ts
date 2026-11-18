// The words of the office flows that depend on counts (offices-and-teams-ux §6.2, §6.4). Pure, so the copy is testable.

export const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "Kai, Juno and Nadia". */
export function nameList(names: string[]): string {
	if (names.length <= 1) return names[0] ?? '';
	return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

export interface RemoveFacts {
	office: string;
	organization: string;
	agents: number;
	tasks: number;
	offline: boolean;
}

/** The Remove office confirmation (§6.4): the office, its agents (which become unplaced), their tasks and the offline case. */
export function removeOfficeText(f: RemoveFacts): string {
	const parts = [`Remove ${f.office} from ${f.organization}? It can't sync again.`];
	if (f.agents > 0) {
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
