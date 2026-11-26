// Pure helper for removable active filter pills in the Work queue toolbar.
import { SAVED_FILTERS, type QueueFilters } from '../../filters';
import { PRIORITY_WORD, SEVERITY_WORD, STATUS_WORD, severityWord } from '../../present';
import type { TaskType } from '../../types';

export interface ActiveFilterChip {
	id: string;
	label: string;
	clearPatch: Partial<QueueFilters>;
	syncUrl?: { projectId?: string; assigneeId?: string };
}

export interface ActiveChipContext {
	filters: QueueFilters;
	activeTab: TaskType;
	projects: [string, string][];
	teams: [string, string][];
	people: [string, string][];
}

function lookup(entries: [string, string][], id: string, noneLabel: string): string {
	if (id === 'none') return noneLabel;
	return entries.find(([key]) => key === id)?.[1] ?? id;
}

function savedLabel(saved: QueueFilters['saved'], activeTab: TaskType): string {
	const def = SAVED_FILTERS.find((s) => s.id === saved);
	if (activeTab === 'request' && def?.requestLabel) return def.requestLabel;
	return def?.label ?? saved;
}

function buildStatusChips(filters: QueueFilters, activeTab: TaskType): ActiveFilterChip[] {
	const chips: ActiveFilterChip[] = [];
	if (filters.saved !== 'all') {
		chips.push({
			id: 'saved',
			label: `Scope: ${savedLabel(filters.saved, activeTab)}`,
			clearPatch: { saved: 'all' }
		});
	}
	if (filters.status !== 'all') {
		const word = filters.status === 'open' ? 'Open' : STATUS_WORD[filters.status];
		chips.push({ id: 'status', label: `Status: ${word}`, clearPatch: { status: 'all' } });
	}
	if (filters.priority !== 'all') {
		chips.push({
			id: 'priority',
			label: `Priority: ${PRIORITY_WORD[filters.priority]}`,
			clearPatch: { priority: 'all' }
		});
	}
	if (filters.severity !== 'all') {
		const word =
			filters.severity === 'unset' ? severityWord(null) : SEVERITY_WORD[filters.severity];
		chips.push({ id: 'severity', label: `Severity: ${word}`, clearPatch: { severity: 'all' } });
	}
	return chips;
}

function buildEntityChips(ctx: ActiveChipContext): ActiveFilterChip[] {
	const { filters, projects, teams, people } = ctx;
	const chips: ActiveFilterChip[] = [];
	if (filters.projectId) {
		chips.push({
			id: 'project',
			label: `Project: ${lookup(projects, filters.projectId, 'No project')}`,
			clearPatch: { projectId: '' },
			syncUrl: { projectId: '' }
		});
	}
	if (filters.assignedTeamId) {
		chips.push({
			id: 'assignedTeam',
			label: `Team: ${lookup(teams, filters.assignedTeamId, 'Unassigned team')}`,
			clearPatch: { assignedTeamId: '' }
		});
	}
	if (filters.teamId) {
		chips.push({
			id: 'team',
			label: `Accountable: ${lookup(teams, filters.teamId, 'None')}`,
			clearPatch: { teamId: '' }
		});
	}
	if (filters.assigneeId) {
		chips.push({
			id: 'assignee',
			label: `Assignee: ${lookup(people, filters.assigneeId, 'Unassigned')}`,
			clearPatch: { assigneeId: '' },
			syncUrl: { assigneeId: '' }
		});
	}
	if (filters.reporterId) {
		chips.push({
			id: 'reporter',
			label: `Reporter: ${lookup(people, filters.reporterId, 'None')}`,
			clearPatch: { reporterId: '' }
		});
	}
	if (filters.query.trim()) {
		chips.push({
			id: 'query',
			label: `Search: "${filters.query.trim()}"`,
			clearPatch: { query: '' }
		});
	}
	return chips;
}

export function buildActiveFilterChips(ctx: ActiveChipContext): ActiveFilterChip[] {
	return [...buildStatusChips(ctx.filters, ctx.activeTab), ...buildEntityChips(ctx)];
}
