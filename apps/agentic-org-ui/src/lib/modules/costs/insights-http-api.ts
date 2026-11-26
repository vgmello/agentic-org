import type { OrgApi } from '$lib/modules/organization/api';
import type { WorkItemsApi } from '$lib/modules/workitems/api';
import type { TaskRow } from '$lib/modules/workitems/types';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { toAppError, type AppError } from '$lib/proto';
import type { HttpCostsReadApi } from './http-api';
import type {
	InsightBreakdownRow,
	InsightNotice,
	OrgInsightProjectRow,
	OrganizationInsights,
	PeriodRequest,
	ProjectInsights,
	ResolvedPeriod,
	SpeedKindRow,
	UsageTotals
} from './types';

type Rec = Record<string, unknown>;

interface GatewayProjectRecord {
	id: string;
	number: number;
	slug: string;
	name: string;
	owner_team_id?: string;
	archived?: boolean;
}

const asRec = (v: unknown): Rec | undefined =>
	v && typeof v === 'object' && !Array.isArray(v) ? (v as Rec) : undefined;
const asStr = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);
const asNum = (v: unknown): number | undefined =>
	typeof v === 'number' && Number.isFinite(v) ? v : undefined;

export function emptyUsageTotals(): UsageTotals {
	return {
		records: 0,
		knownCostMicroUsd: null,
		knownTokens: null,
		cacheReadTokens: null,
		tokenIncompleteRecords: 0,
		costIncompleteRecords: 0,
		unpricedRecords: 0,
		unknownCostRecords: 0,
		includesApiEquivalent: false
	};
}

function parseProjectRecord(raw: unknown): GatewayProjectRecord | undefined {
	const r = asRec(raw);
	if (!r) return undefined;
	const id = asStr(r.id);
	const name = asStr(r.name);
	if (!id || !name) return undefined;
	return {
		id,
		number: asNum(r.number) ?? 1,
		slug: asStr(r.slug) ?? id,
		name,
		owner_team_id: asStr(r.owner_team_id),
		archived: r.archived === true
	};
}

function fallbackPeriod(period: PeriodRequest, nowIso: string): ResolvedPeriod {
	return {
		kind: period.kind,
		endsAt: nowIso,
		timeZone: period.timeZone,
		toNow: period.kind === 'this_week' || period.kind === 'this_month' || period.kind === 'all_time'
	};
}

function inResolvedPeriod(iso: string | undefined, period: ResolvedPeriod): boolean {
	if (!iso) return false;
	const ts = Date.parse(iso);
	if (!Number.isFinite(ts)) return false;
	if (period.startsAt && ts < Date.parse(period.startsAt)) return false;
	return ts <= Date.parse(period.endsAt);
}

function summarizeWorkCounts(rows: readonly TaskRow[], period: ResolvedPeriod) {
	let tasksDone = 0;
	let bugsDone = 0;
	let requestsScoped = 0;
	let blockedCount = 0;
	for (const row of rows) {
		if (row.type === 'request') requestsScoped += 1;
		if (row.status === 'blocked') blockedCount += 1;
		if (row.status === 'done' && inResolvedPeriod(row.statusAt || row.updatedAt, period)) {
			if (row.type === 'bug') bugsDone += 1;
			else if (row.type === 'task') tasksDone += 1;
		}
	}
	return { tasksDone, bugsDone, requestsScoped, blockedCount };
}

function buildWorkInsights(rows: readonly TaskRow[], projectId?: string): InsightNotice[] {
	const notices: InsightNotice[] = [];
	const blocked = rows.filter((r) => r.status === 'blocked');
	if (blocked.length > 0) {
		notices.push({
			id: 'blocked-work',
			kind: 'waiting',
			tone: 'owner',
			text: `${blocked.length} ${blocked.length === 1 ? 'work item is' : 'work items are'} currently blocked.`,
			actionLabel: 'View work',
			href: projectId ? `/projects/${encodeURIComponent(projectId)}/work` : '/work'
		});
	}
	const openCount = rows.filter((r) => r.status !== 'done' && r.status !== 'cancelled').length;
	if (openCount > 0 && notices.length === 0) {
		notices.push({
			id: 'active-queue',
			kind: 'speed',
			tone: 'neutral',
			text: `${openCount} active ${openCount === 1 ? 'item is' : 'items are'} currently in delivery.`,
			actionLabel: 'Open Work',
			href: projectId ? `/projects/${encodeURIComponent(projectId)}/work` : '/work'
		});
	}
	return notices;
}

function buildSpeedRows(rows: readonly TaskRow[], period: ResolvedPeriod): SpeedKindRow[] {
	const done = rows.filter(
		(r) => r.status === 'done' && inResolvedPeriod(r.statusAt || r.updatedAt, period)
	);
	const taskCount = done.filter((r) => r.type === 'task').length;
	const bugCount = done.filter((r) => r.type === 'bug').length;
	return [
		{
			kind: 'task',
			label: 'Tasks',
			sampleCount: taskCount,
			medianLeadSeconds: null,
			medianCycleSeconds: null,
			medianWaitingSeconds: null
		},
		{
			kind: 'bug',
			label: 'Bugs',
			sampleCount: bugCount,
			medianLeadSeconds: null,
			medianCycleSeconds: null,
			medianWaitingSeconds: null
		}
	];
}

function buildTaskTypeBreakdowns(rows: readonly TaskRow[]): InsightBreakdownRow[] {
	const kinds: readonly { id: TaskRow['type']; label: string }[] = [
		{ id: 'task', label: 'Task' },
		{ id: 'bug', label: 'Bug' },
		{ id: 'request', label: 'Request' }
	];
	const out: InsightBreakdownRow[] = [];
	for (const k of kinds) {
		const matching = rows.filter((r) => r.type === k.id);
		if (matching.length === 0) continue;
		const done = matching.filter((r) => r.status === 'done').length;
		out.push({
			id: `kind:${k.id}`,
			key: { dimension: 'task_type', taskType: k.id },
			label: k.label,
			totals: emptyUsageTotals(),
			itemsDone: done,
			costPerTaskMicroUsd: null,
			medianLeadSeconds: null,
			medianCycleSeconds: null,
			sampleCount: done
		});
	}
	return out;
}

function buildTeamBreakdowns(rows: readonly TaskRow[]): InsightBreakdownRow[] {
	const byTeam = new Map<string, { name: string; rows: TaskRow[] }>();
	for (const r of rows) {
		if (!r.team) continue;
		const entry = byTeam.get(r.team.id) ?? { name: r.team.name, rows: [] };
		entry.rows.push(r);
		byTeam.set(r.team.id, entry);
	}
	return [...byTeam.entries()]
		.map(([teamId, info]): InsightBreakdownRow => {
			const done = info.rows.filter((r) => r.status === 'done').length;
			return {
				id: `team:${teamId}`,
				key: { dimension: 'team', teamId },
				label: info.name,
				teamId,
				totals: emptyUsageTotals(),
				itemsDone: done,
				costPerTaskMicroUsd: null,
				medianLeadSeconds: null,
				medianCycleSeconds: null,
				sampleCount: done
			};
		})
		.sort((a, b) => a.label.localeCompare(b.label));
}

function buildConsumerBreakdowns(rows: readonly TaskRow[]): InsightBreakdownRow[] {
	const byMember = new Map<string, { name: string; rows: TaskRow[] }>();
	for (const r of rows) {
		if (!r.assignee) continue;
		const entry = byMember.get(r.assignee.id) ?? { name: r.assignee.name, rows: [] };
		entry.rows.push(r);
		byMember.set(r.assignee.id, entry);
	}
	return [...byMember.entries()]
		.map(([memberId, info]): InsightBreakdownRow => {
			const done = info.rows.filter((r) => r.status === 'done').length;
			return {
				id: `consumer:${memberId}`,
				key: { dimension: 'consumer', memberId },
				label: info.name,
				memberId,
				totals: emptyUsageTotals(),
				itemsDone: done,
				costPerTaskMicroUsd: null,
				medianLeadSeconds: null,
				medianCycleSeconds: null,
				sampleCount: done
			};
		})
		.sort((a, b) => a.label.localeCompare(b.label));
}

export interface HttpInsightsReadApi {
	organizationInsights(
		organizationId: string,
		period: PeriodRequest
	): Promise<OrganizationInsights>;
	projectInsights(
		organizationId: string,
		projectId: string,
		period: PeriodRequest
	): Promise<ProjectInsights>;
}

export function createHttpInsightsReadApi({
	http,
	orgApi,
	costsReadApi,
	workItemsApi,
	now = () => new Date().toISOString()
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	costsReadApi: Pick<HttpCostsReadApi, 'organizationCost'>;
	workItemsApi: Pick<WorkItemsApi, 'queue'>;
	now?: () => string;
}): HttpInsightsReadApi {
	async function readProjects(organizationId: string): Promise<GatewayProjectRecord[]> {
		try {
			const reply: GatewayReply = await http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/projects?include_archived=true`
			);
			const res = unwrapGatewayResult(reply, 'Projects could not be loaded.');
			return Array.isArray(res.projects)
				? res.projects.map(parseProjectRecord).filter((p): p is GatewayProjectRecord => Boolean(p))
				: [];
		} catch {
			return [];
		}
	}

	async function readWorkRows(organizationId: string): Promise<TaskRow[]> {
		try {
			const queue = await workItemsApi.queue(organizationId, { kind: 'organization' });
			return queue.rows;
		} catch {
			return [];
		}
	}

	async function safeOrgCost(organizationId: string, period: PeriodRequest) {
		try {
			return await costsReadApi.organizationCost(organizationId, period);
		} catch (e) {
			const err = toAppError(e);
			if (err.code === 'not_authorized') throw err;
			return undefined;
		}
	}

	return Object.freeze({
		async organizationInsights(
			organizationId: string,
			period: PeriodRequest
		): Promise<OrganizationInsights> {
			const [orgCost, projects, workRows, orgView] = await Promise.all([
				safeOrgCost(organizationId, period),
				readProjects(organizationId),
				readWorkRows(organizationId),
				orgApi.view(organizationId).catch(() => undefined)
			]);
			const nowIso = now();
			const resolved = orgCost?.period ?? fallbackPeriod(period, nowIso);
			const teamsById = new Map((orgView?.snapshot.teams ?? []).map((t) => [t.id, t.name]));
			const counts = summarizeWorkCounts(workRows, resolved);

			const projectRows: OrgInsightProjectRow[] = projects.map((proj) => {
				const projWork = workRows.filter((w) => w.project?.id === proj.id);
				const projDone = projWork.filter(
					(w) => w.status === 'done' && inResolvedPeriod(w.statusAt || w.updatedAt, resolved)
				).length;
				const empty = emptyUsageTotals();
				return {
					projectId: proj.id,
					projectNumber: proj.number,
					name: proj.name,
					...(proj.archived ? { archived: true } : {}),
					...(proj.owner_team_id && teamsById.get(proj.owner_team_id)
						? { accountableTeam: teamsById.get(proj.owner_team_id) }
						: {}),
					toDate: empty,
					thisMonth: empty,
					lastMonth: empty,
					periodTotals: empty,
					changePercent: null,
					itemsDone: projDone,
					medianLeadSeconds: null,
					sampleCount: projDone
				};
			});

			return {
				period: resolved,
				totals: orgCost?.totals ?? emptyUsageTotals(),
				work: {
					tasksDone: counts.tasksDone,
					bugsDone: counts.bugsDone,
					requestsScoped: counts.requestsScoped,
					waitingOnYouSeconds: null,
					cancelledCostMicroUsd: null,
					cancelledRecords: 0
				},
				trend: [],
				insights: buildWorkInsights(workRows),
				projects: projectRows,
				coverage: orgCost?.coverage ?? { checkpointAt: nowIso, gaps: [] },
				viewer: { scope: 'organization', canExport: false }
			};
		},

		async projectInsights(
			organizationId: string,
			projectId: string,
			period: PeriodRequest
		): Promise<ProjectInsights> {
			const [orgCost, projects, workRows] = await Promise.all([
				safeOrgCost(organizationId, period),
				readProjects(organizationId),
				readWorkRows(organizationId)
			]);
			const proj = projects.find((p) => p.id === projectId || p.slug === projectId);
			if (!proj) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That project could not be found.'
				} satisfies AppError;
			}
			const nowIso = now();
			const resolved = orgCost?.period ?? fallbackPeriod(period, nowIso);
			const scopedRows = workRows.filter((w) => w.project?.id === proj.id);
			const periodCounts = summarizeWorkCounts(scopedRows, resolved);
			const lifetimeTasksDone = scopedRows.filter(
				(w) => w.type === 'task' && w.status === 'done'
			).length;
			const lifetimeBugsDone = scopedRows.filter(
				(w) => w.type === 'bug' && w.status === 'done'
			).length;

			return {
				project: { id: proj.id, name: proj.name, number: proj.number, slug: proj.slug },
				period: resolved,
				investment: {
					toDate: emptyUsageTotals(),
					sinceAt: null,
					tasksDone: lifetimeTasksDone,
					bugsDone: lifetimeBugsDone
				},
				totals: emptyUsageTotals(),
				work: {
					tasksDone: periodCounts.tasksDone,
					bugsDone: periodCounts.bugsDone,
					requestsScoped: periodCounts.requestsScoped,
					waitingOnYouSeconds: null,
					cancelledCostMicroUsd: null,
					cancelledRecords: 0
				},
				trend: [],
				insights: buildWorkInsights(scopedRows, proj.id),
				breakdowns: {
					byTeam: buildTeamBreakdowns(scopedRows),
					byConsumer: buildConsumerBreakdowns(scopedRows),
					byModel: [],
					byTaskType: buildTaskTypeBreakdowns(scopedRows),
					byWorkType: []
				},
				releaseCaps: [],
				speed: buildSpeedRows(scopedRows, resolved),
				coverage: orgCost?.coverage ?? { checkpointAt: nowIso, gaps: [] },
				viewer: { scope: 'organization', canExport: false }
			};
		}
	});
}
