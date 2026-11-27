import type { OrgSnapshot } from '$lib/proto';
import { sumRecords } from './http-full-decode';
import type {
	BreakdownKey,
	BreakdownRow,
	MemberProjectSpendRow,
	MemberTaskSpendRow,
	ProjectBreakdownKey,
	UsageRecord
} from './types';

function groupRecordsByProject(records: readonly UsageRecord[], snapshot: OrgSnapshot | undefined) {
	const groups = new Map<string, { label: string; archived?: boolean; items: UsageRecord[] }>();
	const notOnTask: UsageRecord[] = [];
	for (const rec of records) {
		if (!rec.attribution.project) {
			notOnTask.push(rec);
			continue;
		}
		const pid = rec.attribution.project.id;
		const proj = snapshot?.projects?.find((p) => p.id === pid);
		const g = groups.get(pid) ?? {
			label: proj?.name ?? rec.attribution.project.name,
			archived: proj?.archived,
			items: []
		};
		g.items.push(rec);
		groups.set(pid, g);
	}
	return { groups, notOnTask };
}

function buildProjectBreakdown(
	records: readonly UsageRecord[],
	snapshot: OrgSnapshot | undefined
): BreakdownRow[] {
	const { groups, notOnTask } = groupRecordsByProject(records, snapshot);
	const byProject: BreakdownRow[] = [...groups.entries()].map(([projectId, g]) => ({
		id: `project:${projectId}`,
		key: { dimension: 'project', projectId },
		label: g.label,
		projectId,
		...(g.archived ? { archived: true } : {}),
		totals: sumRecords(g.items)
	}));
	if (notOnTask.length > 0) {
		byProject.push({
			id: 'project:not-on-task',
			key: { dimension: 'project', notOnTask: true },
			label: 'Not on a task',
			totals: sumRecords(notOnTask)
		});
	}
	return byProject;
}

function buildConsumerBreakdown(records: readonly UsageRecord[]): BreakdownRow[] {
	const groups = new Map<
		string,
		{ label: string; memberId?: string; archived?: boolean; oneOff?: boolean; items: UsageRecord[] }
	>();
	for (const rec of records) {
		if (rec.consumer.kind === 'agent') {
			const mid = rec.consumer.memberId;
			const g = groups.get(mid) ?? {
				label: rec.consumer.name,
				memberId: mid,
				archived: rec.consumer.former,
				items: []
			};
			g.items.push(rec);
			groups.set(mid, g);
		} else {
			const g = groups.get('__one_off__') ?? {
				label: 'One-off runs',
				oneOff: true,
				items: []
			};
			g.items.push(rec);
			groups.set('__one_off__', g);
		}
	}
	return [...groups.entries()].map(([k, g]) => ({
		id: g.oneOff ? 'consumer:one-off' : `consumer:${k}`,
		key: g.oneOff
			? { dimension: 'consumer', oneOff: true }
			: { dimension: 'consumer', memberId: k },
		label: g.label,
		...(g.memberId ? { memberId: g.memberId } : {}),
		...(g.archived ? { archived: true } : {}),
		totals: sumRecords(g.items)
	}));
}

function buildWorkTypeBreakdown(records: readonly UsageRecord[]): BreakdownRow[] {
	const groups = new Map<'build' | 'run' | 'not_recorded', UsageRecord[]>();
	for (const rec of records) {
		const wt = rec.workType ?? 'not_recorded';
		const list = groups.get(wt) ?? [];
		list.push(rec);
		groups.set(wt, list);
	}
	const wtLabels: Record<'build' | 'run' | 'not_recorded', string> = {
		build: 'Build (R&D)',
		run: 'Run',
		not_recorded: 'Not recorded'
	};
	return (['build', 'run', 'not_recorded'] as const)
		.filter((wt) => (groups.get(wt)?.length ?? 0) > 0)
		.map((wt) => ({
			id: `work-type:${wt}`,
			key: { dimension: 'work_type', workType: wt },
			label: wtLabels[wt],
			totals: sumRecords(groups.get(wt) ?? [])
		}));
}

export function buildOrgBreakdowns(
	records: readonly UsageRecord[],
	snapshot: OrgSnapshot | undefined
): {
	byProject: BreakdownRow[];
	byConsumer: BreakdownRow[];
	byWorkType: BreakdownRow[];
} {
	return {
		byProject: buildProjectBreakdown(records, snapshot),
		byConsumer: buildConsumerBreakdown(records),
		byWorkType: buildWorkTypeBreakdown(records)
	};
}

export function buildMemberBreakdowns(
	records: readonly UsageRecord[],
	snapshot: OrgSnapshot | undefined
): {
	byProject: MemberProjectSpendRow[];
	topTasks: MemberTaskSpendRow[];
} {
	const { groups, notOnTask } = groupRecordsByProject(records, snapshot);
	const byProject: MemberProjectSpendRow[] = [...groups.entries()].map(([projectId, g]) => ({
		id: `project:${projectId}`,
		projectId,
		label: g.label,
		...(g.archived ? { archived: true } : {}),
		totals: sumRecords(g.items)
	}));
	if (notOnTask.length > 0) {
		byProject.push({
			id: 'project:not-on-task',
			label: 'Not on a task',
			notOnTask: true,
			totals: sumRecords(notOnTask)
		});
	}

	const taskMap = new Map<
		string,
		{ title: string; project?: { id: string; name: string }; items: UsageRecord[] }
	>();
	for (const rec of records) {
		if (!rec.attribution.task) continue;
		const ref = rec.attribution.task.ref;
		const g = taskMap.get(ref) ?? {
			title: rec.attribution.task.title,
			project: rec.attribution.project,
			items: []
		};
		g.items.push(rec);
		taskMap.set(ref, g);
	}
	const topTasks: MemberTaskSpendRow[] = [...taskMap.entries()]
		.map(([taskRef, g]) => ({
			taskRef,
			title: g.title,
			...(g.project ? { projectId: g.project.id, projectName: g.project.name } : {}),
			totals: sumRecords(g.items)
		}))
		.sort((a, b) => (b.totals.knownCostMicroUsd ?? 0) - (a.totals.knownCostMicroUsd ?? 0))
		.slice(0, 10);

	return { byProject, topTasks };
}

export function matchesBreakdownKey(rec: UsageRecord, key: BreakdownKey | { all: true }): boolean {
	if ('all' in key) return true;
	if (key.dimension === 'project') {
		if ('notOnTask' in key) return !rec.attribution.project;
		return rec.attribution.project?.id === key.projectId;
	}
	if (key.dimension === 'consumer') {
		if ('oneOff' in key) return rec.consumer.kind === 'one_off';
		return rec.consumer.kind === 'agent' && rec.consumer.memberId === key.memberId;
	}
	return (rec.workType ?? 'not_recorded') === key.workType;
}

export function matchesProjectKey(rec: UsageRecord, key: ProjectBreakdownKey): boolean {
	if ('all' in key) return true;
	if (key.dimension === 'consumer') {
		if ('oneOff' in key) return rec.consumer.kind === 'one_off';
		return rec.consumer.kind === 'agent' && rec.consumer.memberId === key.memberId;
	}
	if (key.dimension === 'work_type') {
		return (rec.workType ?? 'not_recorded') === key.workType;
	}
	if (key.dimension === 'model') {
		return (rec.model ?? null) === key.model && (rec.provider ?? null) === key.provider;
	}
	if (key.dimension === 'team') {
		if ('notOnTask' in key) return !rec.attribution.team;
		return rec.attribution.team?.id === key.teamId;
	}
	return true;
}
