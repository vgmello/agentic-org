// What the Processes screen shows for a place (organization, a team, a project or an agent): the set the engine computes there,
// and the rows' words. Pure.

import { resolve } from './resolve';
import { BRANCH_KEY, branchRule, type RuleState } from './rules';
import type { AgentRef, ProcessWorld } from './world';
import type { EffectiveSet, ProjectRef } from './types';

export type Place = { kind: 'organization' } | { kind: 'team'; id: string } | { kind: 'project'; key: string } | { kind: 'agent'; id: string };

export interface PlaceView {
	title: string;
	/** In words: what this set is ("Processes agents follow in Atlas"). */
	subtitle: string;
	set: EffectiveSet;
	project?: ProjectRef;
	agent?: AgentRef;
	/** The one open branch rule's state where there is a project. */
	rule?: RuleState;
	/** An agent with no current work: only the organization's and its own processes. */
	noCurrentWork: boolean;
}

export const placeKey = (p: Place): string => (p.kind === 'organization' ? 'organization' : p.kind === 'team' ? `team:${p.id}` : p.kind === 'project' ? `project:${p.key}` : `agent:${p.id}`);
export function parsePlace(text: string | null): Place {
	const at = (text ?? '').indexOf(':');
	const kind = at < 0 ? (text ?? '') : (text ?? '').slice(0, at);
	const id = at < 0 ? '' : (text ?? '').slice(at + 1);
	if (kind === 'team' && id) return { kind: 'team', id };
	if (kind === 'project' && id) return { kind: 'project', key: id };
	if (kind === 'agent' && id) return { kind: 'agent', id };
	return { kind: 'organization' };
}

/** The place if it exists in this world; otherwise the organization (a stale or mistyped link). */
export function knownPlace(world: ProcessWorld, place: Place): { place: Place; missing: boolean } {
	const found = place.kind === 'organization' || (place.kind === 'team' && world.teams.some((t) => t.id === place.id)) || (place.kind === 'project' && world.projects.some((p) => p.key === place.key)) || (place.kind === 'agent' && world.agents.some((a) => a.id === place.id));
	return found ? { place, missing: false } : { place: { kind: 'organization' }, missing: true };
}

/** The one open branch rule's line, only where its process is part of the set (with none, there is nothing to say about it). */
const ruleOf = (set: EffectiveSet, project: string): { rule?: RuleState } => {
	const present = set.entries.some((e) => e.key === BRANCH_KEY) || set.turnedOff.some((t) => t.key === BRANCH_KEY);
	return present ? { rule: branchRule(set, project) } : {};
};

export function viewOf(world: ProcessWorld, place: Place): PlaceView {
	const base = { docs: world.docs, teams: world.teams };
	if (place.kind === 'team') {
		const team = world.teams.find((t) => t.id === place.id);
		// A team's view: the organization's processes with the team chain's, as a project of that team would have them (no project files).
		const project: ProjectRef = { key: '', name: team?.name ?? 'Team', teamIds: [place.id] };
		return { title: `Processes · ${team?.name ?? 'Team'} (team)`, subtitle: `What agents follow when they work for ${team?.name ?? 'this team'}: the organization's, its parent teams' and its own.`, set: resolve({ ...base, project, approval: world.approval(project) }), noCurrentWork: false };
	}
	if (place.kind === 'project') {
		const project = world.projects.find((p) => p.key === place.key);
		const set = resolve({ ...base, ...(project ? { project, approval: world.approval(project) } : {}) });
		return { title: `Processes · ${project?.name ?? 'Project'} (project)`, subtitle: `What agents follow when they work in ${project?.name ?? 'this project'}.`, set, ...(project ? { project, ...ruleOf(set, project.name) } : {}), noCurrentWork: false };
	}
	if (place.kind === 'agent') {
		const agent = world.agents.find((a) => a.id === place.id);
		const project = agent?.workingOn ? world.projects.find((p) => p.key === agent.workingOn) : undefined;
		const set = resolve({ ...base, ...(project ? { project, approval: world.approval(project) } : {}), agentId: place.id });
		return {
			title: `Processes · ${agent?.name ?? 'Agent'} (agent)`,
			subtitle: project ? `What ${agent?.name} follows for its current work in ${project.name}.` : `No current work. Showing the organization's processes and its own. Team and project processes apply when it starts a task.`,
			set,
			...(project ? { project, ...ruleOf(set, project.name) } : {}),
			...(agent ? { agent } : {}),
			noCurrentWork: !project
		};
	}
	return { title: 'Processes · Organization', subtitle: 'How agents work in this organization. Teams and projects can override or add to these.', set: resolve(base), noCurrentWork: false };
}
