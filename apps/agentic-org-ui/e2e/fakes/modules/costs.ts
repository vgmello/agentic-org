// Mock for src/lib/modules/costs/api.ts under `bun run dev:mock`. One list of usage records (JSON in
// e2e/fakes/data/costs) is the only source: every total, breakdown, budget and drill-down is computed from it, so the
// figures always add up (COST-4). The clock is fixed at the data's `now`, so periods and budgets don't drift; prices
// use the real date, so "today or later" means today. Names of agents, projects and teams come from the org mock, so
// IDs match the other pages. Added rates are kept in memory until the page reloads.
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (reads fail) · empty (no records, no prices) · unavailable · viewer (not the owner)
//   manager (a project manager: the lead of Web, who sees only the investment of Web's projects)
//   no-zone (the budget zone isn't known) · save-fails (adding a rate fails, verified) · save-unknown (the first add's
//   outcome is unknown; it was in fact applied) · records-fail (the records sheet fails to load)
import { orgApi } from '$lib/modules/organization/api';
import { offsetMinutes } from '$lib/modules/preferences/time';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { CostsApi } from '../../../src/lib/modules/costs/api';
import { OUTCOME_UNKNOWN } from '../../../src/lib/modules/costs/outcome';
import type {
	AgentBudget,
	BreakdownKey,
	BreakdownRow,
	BudgetPeriodState,
	BudgetUnit,
	InsightBreakdownRow,
	InsightNotice,
	MemberCostOverview,
	MemberProjectSpendRow,
	MemberRecordsFilter,
	MemberTaskSpendRow,
	OrgInsightProjectRow,
	OrganizationInsights,
	PeriodRequest,
	PriceVersion,
	ProjectBreakdownKey,
	ProjectInsights,
	ReleaseCapRow,
	ResolvedPeriod,
	SpeedKindRow,
	SubscriptionModelUsage,
	SubscriptionUsageOverview,
	SubscriptionUsageSnapshot,
	TrendBucket,
	UsageRecord,
	UsageTotals
} from '../../../src/lib/modules/costs/types';
import { mockMode, POC_ORG_ID } from '../mock-mode';
import { pocUsageRecords, type PocStoredUsageRecord } from '../poc-dataset';
import usage from '../data/costs/usage.json' with { type: 'json' };
import priceData from '../data/costs/prices.json' with { type: 'json' };
import budgetData from '../data/costs/budgets.json' with { type: 'json' };
import projectsData from '../data/projects/projects.json' with { type: 'json' };
import releasesData from '../data/releases/releases.json' with { type: 'json' };

type StoredRecord = PocStoredUsageRecord;
type StoredPrice = Omit<PriceVersion, 'used'> & { scheduled?: boolean };
type StoredBudget = {
	unit: BudgetUnit;
	limit: number;
	warned?: boolean;
	hold?: 'stop_pending' | 'held';
	extended?: boolean;
};

const NOW = new Date(usage.now);
const PAGE = 20;
const all = usage.records as StoredRecord[];
const superseded = new Set(all.map((r) => r.supersedesId).filter(Boolean));
/** The effective ledger: records nothing has replaced. */
const effective = all.filter((r) => !superseded.has(r.id));
const pocLedger: StoredRecord[] = pocUsageRecords();

function ledgerFor(organizationId: string): StoredRecord[] {
	return organizationId === POC_ORG_ID ? pocLedger : effective;
}

function coverageGapsFor(organizationId: string) {
	if (on('empty') || organizationId === POC_ORG_ID) return [];
	return usage.gaps;
}

/** The scheduled rate always starts on the first of next month, so it stays scheduled whatever the date. */
function nextMonthStart(): string {
	const now = new Date();
	return new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() + 1, 1, 5)).toISOString();
}
let prices: StoredPrice[] = (structuredClone(priceData) as StoredPrice[]).map((p) =>
	p.scheduled ? { ...p, effectiveFrom: nextMonthStart() } : p
);
const operations = new Map<string, PriceVersion>();
let unknownOnce = false;
type OrgBudgetMap = Record<string, { daily?: StoredBudget; monthly?: StoredBudget }>;
const budgetsByOrg = new Map<string, OrgBudgetMap>();
function budgetsFor(organizationId: string): OrgBudgetMap {
	let map = budgetsByOrg.get(organizationId);
	if (!map) {
		map =
			organizationId === POC_ORG_ID
				? {
						ada: { monthly: { unit: 'usd', limit: 40_000_000, warned: true } },
						eden: { monthly: { unit: 'usd', limit: 30_000_000 } },
						jules: { monthly: { unit: 'usd', limit: 40_000_000, warned: true, hold: 'held' } },
						kai: { monthly: { unit: 'usd', limit: 35_000_000 } },
						rowan: { monthly: { unit: 'usd', limit: 35_000_000, warned: true } }
					}
				: structuredClone(budgetData.budgets as OrgBudgetMap);
		budgetsByOrg.set(organizationId, map);
	}
	return map;
}
const budgetOperations = new Map<string, { applied: true; message: string }>();
let budgetUnknownOnce = false;
let budgetActionUnknownOnce = false;

const projectMeta = projectsData.projects as Record<
	string,
	{
		number: number;
		slug: string;
		tasks: { todo: number; inProgress: number; blocked: number; done: number; cancelled: number };
	}
>;
const releaseMeta = releasesData.projects as Record<
	string,
	{
		releases: {
			id: string;
			number: number;
			versionLabel?: string;
			title: string;
			status: string;
			updatedAt?: string;
		}[];
	}
>;

const on = (name: string) => mockMode().split(',').includes(name);
const fail = (code: string, message: string, extra: Record<string, unknown> = {}) =>
	Object.assign(new Error(message), { source: 'api', code, message, ...extra });

async function read() {
	if (on('slow')) await new Promise((r) => setTimeout(r, 2500));
	if (on('unavailable')) await unavailableCall();
	if (on('fails') || on('costs-fail'))
		throw fail('internal', "HQ didn't answer. Check that the office is running, then try again.");
}
function ownerOnly() {
	if (on('viewer')) throw fail('not_authorized', "Only the organization's owner sees its cost.");
}

// ---- Time ---------------------------------------------------------------------------------------------------------

/** The instant a calendar day starts in `zone` (DST applied for that day). */
function dayStart(y: number, m: number, d: number, zone: string): Date {
	const guess = Date.UTC(y, m - 1, d);
	let at = guess - offsetMinutes(new Date(guess), zone) * 60_000;
	at = guess - offsetMinutes(new Date(at), zone) * 60_000;
	return new Date(at);
}
function localParts(date: Date, zone: string) {
	const p = Object.fromEntries(
		new Intl.DateTimeFormat('en-GB', {
			timeZone: zone,
			year: 'numeric',
			month: '2-digit',
			day: '2-digit',
			weekday: 'short'
		})
			.formatToParts(date)
			.map((x) => [x.type, x.value])
	);
	return { y: Number(p.year), m: Number(p.month), d: Number(p.day), weekday: p.weekday as string };
}
const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const parseDay = (key: string) => key.split('-').map(Number) as [number, number, number];

function resolvePeriod(request: PeriodRequest): {
	period: ResolvedPeriod;
	previous?: ResolvedPeriod;
} {
	const zone = request.timeZone;
	const now = localParts(NOW, zone);
	const resolved = (
		kind: PeriodRequest['kind'],
		start: Date | undefined,
		end: Date,
		toNow: boolean
	) => ({
		kind,
		...(start ? { startsAt: start.toISOString() } : {}),
		endsAt: end.toISOString(),
		timeZone: zone,
		toNow
	});
	const sameLengthBefore = (start: Date, end: Date) =>
		resolved(
			request.kind,
			new Date(start.getTime() - (end.getTime() - start.getTime())),
			start,
			false
		);
	switch (request.kind) {
		case 'this_week': {
			const back = WEEKDAYS.indexOf(now.weekday);
			const monday = new Date(Date.UTC(now.y, now.m - 1, now.d - back));
			const start = dayStart(
				monday.getUTCFullYear(),
				monday.getUTCMonth() + 1,
				monday.getUTCDate(),
				zone
			);
			return {
				period: resolved('this_week', start, NOW, true),
				previous: sameLengthBefore(start, NOW)
			};
		}
		case 'this_month': {
			const start = dayStart(now.y, now.m, 1, zone);
			return {
				period: resolved('this_month', start, NOW, true),
				previous: sameLengthBefore(start, NOW)
			};
		}
		case 'last_month': {
			const end = dayStart(now.y, now.m, 1, zone);
			const start = dayStart(
				now.m === 1 ? now.y - 1 : now.y,
				now.m === 1 ? 12 : now.m - 1,
				1,
				zone
			);
			const first = localParts(new Date(start.getTime() + 12 * 3_600_000), zone);
			const before = dayStart(
				first.m === 1 ? first.y - 1 : first.y,
				first.m === 1 ? 12 : first.m - 1,
				1,
				zone
			);
			return {
				period: resolved('last_month', start, end, false),
				previous: resolved('last_month', before, start, false)
			};
		}
		case 'custom': {
			if (!request.from)
				throw fail('invalid_request', 'A custom period needs its first day.', { field: 'from' });
			if (request.to && request.to < request.from)
				throw fail('invalid_request', "The last day can't be before the first day.", {
					field: 'to'
				});
			const start = dayStart(...parseDay(request.from), zone);
			let end = NOW;
			let toNow = true;
			if (request.to) {
				const [y, m, d] = parseDay(request.to);
				const after = new Date(Date.UTC(y, m - 1, d + 1));
				const candidate = dayStart(
					after.getUTCFullYear(),
					after.getUTCMonth() + 1,
					after.getUTCDate(),
					zone
				);
				if (candidate < NOW) {
					end = candidate;
					toNow = false;
				}
			}
			return {
				period: resolved('custom', start, end, toNow),
				previous: sameLengthBefore(start, end)
			};
		}
		default:
			return { period: resolved('all_time', undefined, NOW, true) };
	}
}
const within = (r: StoredRecord, p: ResolvedPeriod) =>
	(!p.startsAt || r.occurredAt >= p.startsAt) && r.occurredAt < p.endsAt;

// ---- Totals -------------------------------------------------------------------------------------------------------

function totalsOf(records: StoredRecord[]): UsageTotals {
	let cost: number | null = null;
	let tokens: number | null = null;
	let cacheRead: number | null = null;
	const t: UsageTotals = {
		records: records.length,
		knownCostMicroUsd: null,
		knownTokens: null,
		cacheReadTokens: null,
		tokenIncompleteRecords: 0,
		costIncompleteRecords: 0,
		unpricedRecords: 0,
		unknownCostRecords: 0,
		includesApiEquivalent: false
	};
	for (const r of records) {
		if (r.costMicroUsd !== null) cost = (cost ?? 0) + r.costMicroUsd;
		else if (r.costBasis === 'unpriced') t.unpricedRecords++;
		else t.unknownCostRecords++;
		if (r.tokens) {
			tokens = (tokens ?? 0) + r.tokens[0] + r.tokens[1] + r.tokens[2] + r.tokens[3];
			cacheRead = (cacheRead ?? 0) + r.tokens[2];
		}
		if (r.tokenStatus !== 'complete') t.tokenIncompleteRecords++;
		if (r.costStatus !== 'complete') t.costIncompleteRecords++;
		if (r.apiEquivalent) t.includesApiEquivalent = true;
	}
	return { ...t, knownCostMicroUsd: cost, knownTokens: tokens, cacheReadTokens: cacheRead };
}

const byCost = (a: BreakdownRow, b: BreakdownRow) =>
	(b.totals.knownCostMicroUsd ?? -1) - (a.totals.knownCostMicroUsd ?? -1) ||
	a.label.localeCompare(b.label);

async function names(organizationId: string) {
	const { snapshot } = await orgApi.view(organizationId);
	return {
		member: new Map(snapshot.members.map((m) => [m.id, m.name])),
		project: new Map((snapshot.projects ?? []).map((p) => [p.id, p.name])),
		team: new Map(snapshot.teams.map((t) => [t.id, t.name])),
		agents: snapshot.members.filter((m) => m.kind === 'agent')
	};
}

function matches(r: StoredRecord, key: BreakdownKey | { all: true }): boolean {
	if ('all' in key) return true;
	switch (key.dimension) {
		case 'project':
			return 'notOnTask' in key
				? !r.attribution.projectId
				: r.attribution.projectId === key.projectId;
		case 'consumer':
			return 'oneOff' in key
				? r.consumer.kind === 'one_off'
				: r.consumer.kind === 'agent' && r.consumer.memberId === key.memberId;
		case 'work_type':
			return key.workType === 'not_recorded' ? r.workType === null : r.workType === key.workType;
	}
}

function rowsBy<K extends string>(
	records: StoredRecord[],
	keyOf: (r: StoredRecord) => K,
	make: (key: K, records: StoredRecord[]) => BreakdownRow
): BreakdownRow[] {
	const groups = new Map<K, StoredRecord[]>();
	for (const r of records) groups.set(keyOf(r), [...(groups.get(keyOf(r)) ?? []), r]);
	return [...groups.entries()].map(([key, list]) => make(key, list)).toSorted(byCost);
}

// ---- Prices -------------------------------------------------------------------------------------------------------

const usedPrices = () => new Set(all.map((r) => r.priceId).filter(Boolean));
const toPrice = (p: StoredPrice): PriceVersion => {
	const rest: Omit<StoredPrice, 'scheduled'> & { scheduled?: boolean } = { ...p };
	delete rest.scheduled;
	return { ...rest, used: usedPrices().has(p.id) };
};
function priceLabel(id: string): string | undefined {
	const p = prices.find((x) => x.id === id);
	if (!p) return undefined;
	const day = new Intl.DateTimeFormat('en-GB', {
		day: 'numeric',
		month: 'short',
		year: 'numeric',
		timeZone: 'UTC'
	}).format(new Date(p.effectiveFrom));
	return `${p.provider}/${p.model}, rates from ${day} (version …${p.id.slice(-4)})`;
}
const ISSUES: Record<string, string> = {
	runtime_reports_no_usage: "Antigravity doesn't report usage",
	no_rate_for_cache_write: 'No cache-write rate for this model, so the cost is partial',
	no_price_for_model: 'No price set for this model'
};

function toRecord(r: StoredRecord, n: Awaited<ReturnType<typeof names>>): UsageRecord {
	const a = r.attribution;
	return {
		id: r.id,
		occurredAt: r.occurredAt,
		consumer:
			r.consumer.kind === 'agent'
				? {
						kind: 'agent',
						memberId: r.consumer.memberId,
						name: n.member.get(r.consumer.memberId) ?? r.consumer.memberId
					}
				: { kind: 'one_off', label: r.consumer.label },
		runtime: r.runtime,
		provider: r.provider,
		model: r.model,
		tokens: r.tokens
			? {
					input: r.tokens[0],
					output: r.tokens[1],
					cacheRead: r.tokens[2],
					cacheWrite: r.tokens[3],
					reasoning: r.tokens[4]
				}
			: null,
		tokenStatus: r.tokenStatus,
		costMicroUsd: r.costMicroUsd,
		costBasis: r.costBasis,
		costStatus: r.costStatus,
		apiEquivalent: !!r.apiEquivalent,
		...(r.priceId && priceLabel(r.priceId)
			? { price: { id: r.priceId, label: priceLabel(r.priceId)! } }
			: {}),
		attribution: {
			basis: a.basis,
			...(a.taskRef ? { task: { ref: a.taskRef, title: a.taskTitle ?? '' } } : {}),
			...(a.projectId
				? { project: { id: a.projectId, name: n.project.get(a.projectId) ?? a.projectId } }
				: {}),
			...(a.teamId ? { team: { id: a.teamId, name: n.team.get(a.teamId) ?? a.teamId } } : {})
		},
		workType: r.workType,
		...(r.issue ? { issue: ISSUES[r.issue] ?? r.issue } : {}),
		...(r.correction ? { correction: r.correction } : {})
	};
}

// ---- Budgets ------------------------------------------------------------------------------------------------------

function budgetState(
	stored: StoredBudget,
	records: StoredRecord[],
	period: { key: string; startsAt: Date; endsAt: Date },
	unmeasured: boolean
): BudgetPeriodState {
	const inPeriod = records.filter(
		(r) =>
			r.occurredAt >= period.startsAt.toISOString() && r.occurredAt < period.endsAt.toISOString()
	);
	const totals = totalsOf(inPeriod);
	// Document 17: the one extension raises the base limit L to L + floor(L / 5).
	const limit = stored.extended ? stored.limit + Math.floor(stored.limit / 5) : stored.limit;
	const spent = stored.unit === 'usd' ? (totals.knownCostMicroUsd ?? 0) : (totals.knownTokens ?? 0);
	const complete =
		stored.unit === 'usd'
			? totals.costIncompleteRecords === 0
			: totals.tokenIncompleteRecords === 0;
	return {
		unit: stored.unit,
		limit,
		...(stored.extended ? { baseLimit: stored.limit } : {}),
		spent,
		complete: complete && !unmeasured,
		...(unmeasured
			? { unmeasured: true, incompleteReason: "Antigravity doesn't report usage" }
			: {}),
		period: {
			key: period.key,
			startsAt: period.startsAt.toISOString(),
			endsAt: period.endsAt.toISOString()
		},
		...(stored.hold && spent >= limit ? { hold: stored.hold } : {}),
		...(stored.extended ? { extended: true } : {}),
		...(stored.warned ? { warned: true } : {})
	};
}

// ---- The API ------------------------------------------------------------------------------------------------------

export const costsApi: CostsApi = {
	async prices() {
		await read();
		return structuredClone({
			prices: on('empty') ? [] : prices.map(toPrice),
			viewer: { canManage: !on('viewer') }
		});
	},

	async addPrice(_organizationId, draft) {
		if (on('slow')) await new Promise((r) => setTimeout(r, 2500));
		if (on('unavailable')) await unavailableCall();
		const replay = operations.get(draft.operationId);
		if (replay) return structuredClone(replay);
		if (on('viewer')) throw fail('not_authorized', 'Only the owner adds rates.');
		if (on('save-fails'))
			throw fail('storage_error', "HQ couldn't write the price file. Nothing was added.");
		const today = new Intl.DateTimeFormat('en-CA', { timeZone: draft.timeZone }).format(new Date());
		if (draft.effectiveFrom < today)
			throw fail('invalid_request', 'A new rate takes effect from today or a later date.', {
				field: 'effectiveFrom'
			});
		const effectiveFrom = dayStart(...parseDay(draft.effectiveFrom), draft.timeZone).toISOString();
		if (
			prices.some(
				(p) =>
					p.provider.toLowerCase() === draft.provider.toLowerCase() &&
					p.model.toLowerCase() === draft.model.toLowerCase() &&
					p.effectiveFrom === effectiveFrom
			)
		)
			throw fail(
				'already_exists',
				`${draft.model} already has a rate from that day. Choose another day.`,
				{ field: 'effectiveFrom' }
			);
		const stored: StoredPrice = {
			id: crypto.randomUUID(),
			provider: draft.provider,
			model: draft.model,
			effectiveFrom,
			inputMicroUsdPerMillion: draft.inputMicroUsdPerMillion,
			outputMicroUsdPerMillion: draft.outputMicroUsdPerMillion,
			cacheReadMicroUsdPerMillion: draft.cacheReadMicroUsdPerMillion,
			cacheWriteMicroUsdPerMillion: draft.cacheWriteMicroUsdPerMillion,
			created: { at: new Date().toISOString(), byName: 'Alex Kim' }
		};
		prices = [...prices, stored];
		const price = toPrice(stored);
		operations.set(draft.operationId, price);
		if (on('save-unknown') && !unknownOnce) {
			// The rate was recorded, but the answer never arrived.
			unknownOnce = true;
			throw Object.assign(new Error('The connection to HQ dropped before it answered.'), {
				source: 'app',
				code: OUTCOME_UNKNOWN,
				message: 'The connection to HQ dropped before it answered.'
			});
		}
		return structuredClone(price);
	},

	async priceOperation(_organizationId, operationId) {
		if (on('slow')) await new Promise((r) => setTimeout(r, 2500));
		if (on('unavailable')) await unavailableCall();
		const price = operations.get(operationId);
		return price
			? { state: 'applied', price: structuredClone(price) }
			: { state: 'not_applied', reason: 'HQ has no record of this rate.' };
	},

	async projectInvestment(organizationId, request) {
		await read();
		if (on('viewer'))
			throw fail('not_authorized', "Only the owner and a project's manager see its cost.");
		const { snapshot } = await orgApi.view(organizationId);
		// A project's manager is the lead of its accountable team (the first team listed on it) or of a team above it.
		const above = (teamId: string | undefined): string[] => {
			const team = snapshot.teams.find((t) => t.id === teamId);
			return team ? [team.id, ...above(team.owner_team_id)] : [];
		};
		const owner = (projectId: string) =>
			(snapshot.project_teams ?? []).find((l) => l.project_id === projectId)?.team_id;
		const managed = on('manager');
		const projects = (snapshot.projects ?? []).filter(
			(p) => !managed || above(owner(p.id)).includes('web')
		);
		const month = resolvePeriod({ kind: 'this_month', timeZone: request.timeZone }).period;
		const records = on('empty') ? [] : ledgerFor(organizationId);
		return structuredClone({
			scope: managed ? ('managed' as const) : ('organization' as const),
			thisMonth: month,
			projects: projects.map((p) => {
				const mine = records.filter((r) => r.attribution.projectId === p.id);
				return {
					projectId: p.id,
					toDate: totalsOf(mine),
					thisMonth: totalsOf(mine.filter((r) => within(r, month)))
				};
			}),
			coverage: { checkpointAt: usage.checkpointAt, gaps: coverageGapsFor(organizationId) }
		});
	},

	async organizationCost(organizationId, request) {
		await read();
		ownerOnly();
		const { period, previous } = resolvePeriod(request);
		const n = await names(organizationId);
		const ledger = on('empty') ? [] : ledgerFor(organizationId);
		const records = ledger.filter((r) => within(r, period));
		const totalsPrevious = previous
			? totalsOf(ledger.filter((r) => within(r, previous)))
			: undefined;
		const onTask = records.filter((r) => r.attribution.projectId);
		const notOnTask = records.filter((r) => !r.attribution.projectId);
		const byProject = rowsBy(
			onTask,
			(r) => r.attribution.projectId!,
			(id, list) => ({
				id: `project:${id}`,
				key: { dimension: 'project', projectId: id },
				label: n.project.get(id) ?? id,
				projectId: id,
				...(n.project.has(id) ? {} : { archived: true }),
				totals: totalsOf(list)
			})
		);
		if (notOnTask.length)
			byProject.push({
				id: 'project:not-on-a-task',
				key: { dimension: 'project', notOnTask: true },
				label: 'Not on a task',
				totals: totalsOf(notOnTask),
				overhead: {
					team: totalsOf(notOnTask.filter((r) => r.attribution.basis === 'team_overhead')),
					organization: totalsOf(notOnTask.filter((r) => r.attribution.basis !== 'team_overhead'))
				}
			});
		const byConsumer = rowsBy(
			records,
			(r) => (r.consumer.kind === 'agent' ? r.consumer.memberId : ':one-off'),
			(id, list) =>
				id === ':one-off'
					? {
							id: 'consumer:one-off',
							key: { dimension: 'consumer', oneOff: true },
							label: 'One-off model calls (drafting)',
							totals: totalsOf(list)
						}
					: {
							id: `consumer:${id}`,
							key: { dimension: 'consumer', memberId: id },
							label: n.member.get(id) ?? id,
							memberId: id,
							totals: totalsOf(list)
						}
		);
		const WORK = { build: 'Build', run: 'Run', not_recorded: 'Not recorded' } as const;
		const byWorkType = rowsBy(
			records,
			(r) => r.workType ?? 'not_recorded',
			(id, list) => ({
				id: `work:${id}`,
				key: { dimension: 'work_type', workType: id },
				label: WORK[id],
				totals: totalsOf(list)
			})
		);
		return structuredClone({
			period,
			totals: totalsOf(records),
			...(previous && totalsPrevious
				? { previous: { period: previous, knownCostMicroUsd: totalsPrevious.knownCostMicroUsd } }
				: {}),
			byProject,
			byConsumer,
			byWorkType,
			coverage: {
				checkpointAt: usage.checkpointAt,
				gaps: period.toNow ? coverageGapsFor(organizationId) : []
			}
		});
	},

	async records(organizationId, request, key, cursor) {
		await read();
		ownerOnly();
		if (on('records-fail'))
			throw fail('internal', "HQ didn't answer. Check that the office is running, then try again.");
		const { period } = resolvePeriod(request);
		const n = await names(organizationId);
		const list = (on('empty') ? [] : ledgerFor(organizationId))
			.filter((r) => within(r, period) && matches(r, key))
			.toSorted((a, b) => b.occurredAt.localeCompare(a.occurredAt));
		const offset = Number(cursor ?? 0);
		const page = list.slice(offset, offset + PAGE).map((r) => toRecord(r, n));
		return structuredClone({
			records: page,
			total: list.length,
			...(offset + PAGE < list.length ? { nextCursor: String(offset + PAGE) } : {})
		});
	},

	async agentBudgets(organizationId) {
		await read();
		ownerOnly();
		const n = await names(organizationId);
		const zone = on('no-zone') ? null : budgetData.zone.name;
		const stored = budgetsFor(organizationId);
		const ledger = ledgerFor(organizationId);
		const agents: AgentBudget[] = n.agents
			.map((m) => {
				const mine = ledger.filter(
					(r) => r.consumer.kind === 'agent' && r.consumer.memberId === m.id
				);
				const unmeasured = mine.some((r) => r.runtime === 'antigravity');
				const b = stored[m.id];
				if (!zone || !b || on('empty')) return { memberId: m.id, name: m.name };
				const { day, month } = budgetWindows(zone);
				return {
					memberId: m.id,
					name: m.name,
					...(b.daily ? { daily: budgetState(b.daily, mine, day, unmeasured) } : {}),
					...(b.monthly ? { monthly: budgetState(b.monthly, mine, month, unmeasured) } : {})
				};
			})
			.toSorted((a, b) => a.name.localeCompare(b.name));
		return structuredClone({
			zone: { name: zone, source: budgetData.zone.source as 'owner_account' | 'hq' },
			agents
		});
	},

	async projectInsights(organizationId, projectId, request) {
		await read();
		if (on('viewer'))
			throw fail('not_authorized', "Only the owner and this project's manager see its costs.");
		const { snapshot } = await orgApi.view(organizationId);
		const managedIds = managedProjectIds(snapshot);
		const managed = on('manager');
		if (managed && !managedIds.has(projectId))
			throw fail('not_authorized', "Only the owner and this project's manager see its costs.");
		const proj = (snapshot.projects ?? []).find((p) => p.id === projectId);
		if (!proj) throw fail('not_found', 'Project not found.');
		const { period, previous } = resolvePeriod(request);
		const n = await names(organizationId);
		const allMine = on('empty')
			? []
			: ledgerFor(organizationId).filter((r) => r.attribution.projectId === projectId);
		const records = allMine.filter((r) => within(r, period));
		const prevTotals = previous ? totalsOf(allMine.filter((r) => within(r, previous))) : undefined;
		const meta = projectMeta[projectId];
		const oldest = allMine.reduce<string | null>(
			(acc, r) => (!acc || r.occurredAt < acc ? r.occurredAt : acc),
			null
		);
		const periodDone = countWorkDone(records, meta?.tasks.done ?? 0, period.kind);
		const cancelledRecords = records.filter((r) => isCancelledTask(r));
		const cancelledTotals = totalsOf(cancelledRecords);
		const trend = buildTrend(records, period);
		const breakdowns = buildProjectBreakdowns(records, n);
		const insights = buildProjectInsightsNotices(projectId, proj.name, records, breakdowns);
		const releaseCaps = buildReleaseCaps(projectId, allMine);
		const speed = buildSpeedRows(records);
		return structuredClone({
			project: {
				id: proj.id,
				name: proj.name,
				number: meta?.number ?? 1,
				slug: meta?.slug ?? proj.id
			},
			period,
			...(previous && prevTotals
				? { previous: { period: previous, knownCostMicroUsd: prevTotals.knownCostMicroUsd } }
				: {}),
			investment: {
				toDate: totalsOf(allMine),
				sinceAt: oldest,
				tasksDone: meta?.tasks.done ?? periodDone.tasksDone,
				bugsDone: projectId === 'beacon' ? 4 : projectId === 'atlas' ? 5 : 1
			},
			totals: totalsOf(records),
			work: {
				tasksDone: periodDone.tasksDone,
				bugsDone: periodDone.bugsDone,
				requestsScoped: projectId === 'beacon' ? 3 : projectId === 'atlas' ? 2 : 0,
				waitingOnYouSeconds: records.length
					? projectId === 'beacon'
						? 19 * 3600
						: 11 * 3600
					: null,
				cancelledCostMicroUsd: cancelledRecords.length ? cancelledTotals.knownCostMicroUsd : null,
				cancelledRecords: cancelledRecords.length
			},
			trend,
			insights,
			breakdowns,
			releaseCaps,
			speed,
			coverage: {
				checkpointAt: usage.checkpointAt,
				gaps: period.toNow ? coverageGapsFor(organizationId) : []
			},
			viewer: { scope: managed ? ('managed' as const) : ('organization' as const), canExport: true }
		} satisfies ProjectInsights);
	},

	async projectRecords(organizationId, projectId, request, key, cursor) {
		await read();
		if (on('viewer'))
			throw fail('not_authorized', "Only the owner and this project's manager see its costs.");
		if (on('records-fail'))
			throw fail('internal', "HQ didn't answer. Check that the office is running, then try again.");
		const { snapshot } = await orgApi.view(organizationId);
		if (on('manager') && !managedProjectIds(snapshot).has(projectId))
			throw fail('not_authorized', "Only the owner and this project's manager see its costs.");
		const { period } = resolvePeriod(request);
		const n = await names(organizationId);
		const list = (on('empty') ? [] : ledgerFor(organizationId))
			.filter(
				(r) =>
					r.attribution.projectId === projectId && within(r, period) && matchesProjectKey(r, key)
			)
			.toSorted((a, b) => b.occurredAt.localeCompare(a.occurredAt));
		const offset = Number(cursor ?? 0);
		const page = list.slice(offset, offset + PAGE).map((r) => toRecord(r, n));
		return structuredClone({
			records: page,
			total: list.length,
			...(offset + PAGE < list.length ? { nextCursor: String(offset + PAGE) } : {})
		});
	},

	async organizationInsights(organizationId, request) {
		await read();
		if (on('viewer'))
			throw fail(
				'not_authorized',
				'Only the owner and project managers see organization insights.'
			);
		const { snapshot } = await orgApi.view(organizationId);
		const managed = on('manager');
		const allowedIds = managedProjectIds(snapshot);
		const { period, previous } = resolvePeriod(request);
		const thisMonth = resolvePeriod({ kind: 'this_month', timeZone: request.timeZone }).period;
		const lastMonth = resolvePeriod({ kind: 'last_month', timeZone: request.timeZone }).period;
		const n = await names(organizationId);
		const visibleAll = (on('empty') ? [] : ledgerFor(organizationId)).filter((r) =>
			managed ? !!(r.attribution.projectId && allowedIds.has(r.attribution.projectId)) : true
		);
		const periodRecords = visibleAll.filter((r) => within(r, period));
		const prevTotals = previous
			? totalsOf(visibleAll.filter((r) => within(r, previous)))
			: undefined;
		const visibleProjects = (snapshot.projects ?? []).filter(
			(p) => !managed || allowedIds.has(p.id)
		);
		const ownerTeamName = (projectId: string) => {
			const teamId = (snapshot.project_teams ?? []).find(
				(l) => l.project_id === projectId
			)?.team_id;
			return teamId ? n.team.get(teamId) : undefined;
		};
		const projects: OrgInsightProjectRow[] = visibleProjects.map((p) => {
			const mine = visibleAll.filter((r) => r.attribution.projectId === p.id);
			const mThis = totalsOf(mine.filter((r) => within(r, thisMonth)));
			const mLast = totalsOf(mine.filter((r) => within(r, lastMonth)));
			const pPeriod = totalsOf(mine.filter((r) => within(r, period)));
			const meta = projectMeta[p.id];
			const changePercent =
				mThis.knownCostMicroUsd !== null &&
				mLast.knownCostMicroUsd !== null &&
				mLast.knownCostMicroUsd > 0
					? Math.round(
							((mThis.knownCostMicroUsd - mLast.knownCostMicroUsd) * 100) / mLast.knownCostMicroUsd
						)
					: null;
			const done = countWorkDone(
				mine.filter((r) => within(r, period)),
				meta?.tasks.done ?? 0,
				period.kind
			);
			const itemsDone = done.tasksDone + done.bugsDone;
			return {
				projectId: p.id,
				projectNumber: meta?.number ?? 1,
				name: p.name,
				accountableTeam: ownerTeamName(p.id),
				toDate: totalsOf(mine),
				thisMonth: mThis,
				lastMonth: mLast,
				periodTotals: pPeriod,
				changePercent,
				itemsDone,
				medianLeadSeconds: itemsDone >= 5 ? (p.id === 'atlas' ? 138_240 : 95_040) : null,
				sampleCount: itemsDone
			};
		});
		const notOnTask = managed
			? undefined
			: totalsOf(visibleAll.filter((r) => within(r, thisMonth) && !r.attribution.projectId));
		const totalTotals = totalsOf(periodRecords);
		const cancelled = periodRecords.filter((r) => isCancelledTask(r));
		const insights = buildOrgInsightsNotices(projects, totalTotals);
		return structuredClone({
			period,
			...(previous && prevTotals
				? { previous: { period: previous, knownCostMicroUsd: prevTotals.knownCostMicroUsd } }
				: {}),
			totals: totalTotals,
			work: {
				tasksDone: projects.reduce((acc, p) => acc + Math.max(0, p.itemsDone - 1), 0),
				bugsDone: projects.reduce((acc, p) => acc + (p.itemsDone > 0 ? 1 : 0), 0),
				requestsScoped: periodRecords.length ? 4 : 0,
				waitingOnYouSeconds: periodRecords.length ? 30 * 3600 : null,
				cancelledCostMicroUsd: cancelled.length ? totalsOf(cancelled).knownCostMicroUsd : null,
				cancelledRecords: cancelled.length
			},
			trend: buildTrend(periodRecords, period),
			insights,
			projects,
			...(notOnTask && notOnTask.records > 0 ? { notOnTask } : {}),
			coverage: {
				checkpointAt: usage.checkpointAt,
				gaps: period.toNow ? coverageGapsFor(organizationId) : []
			},
			viewer: { scope: managed ? ('managed' as const) : ('organization' as const), canExport: true }
		} satisfies OrganizationInsights);
	},

	async exportRecords(organizationId, scope, request) {
		await read();
		if (on('viewer'))
			throw fail('not_authorized', 'You are not authorized to export cost records.');
		const { snapshot } = await orgApi.view(organizationId);
		const managed = on('manager');
		const allowedIds = managedProjectIds(snapshot);
		if (scope.kind === 'project' && managed && !allowedIds.has(scope.projectId))
			throw fail(
				'not_authorized',
				"Only the owner and this project's manager may export its costs."
			);
		const { period } = resolvePeriod(request);
		const n = await names(organizationId);
		const list = (on('empty') ? [] : ledgerFor(organizationId))
			.filter((r) => {
				if (!within(r, period)) return false;
				if (scope.kind === 'project') return r.attribution.projectId === scope.projectId;
				if (managed) return !!(r.attribution.projectId && allowedIds.has(r.attribution.projectId));
				return true;
			})
			.toSorted((a, b) => b.occurredAt.localeCompare(a.occurredAt))
			.map((r) => toRecord(r, n));
		return structuredClone({
			period,
			records: list,
			coverage: {
				checkpointAt: usage.checkpointAt,
				gaps: period.toNow ? coverageGapsFor(organizationId) : []
			}
		});
	},

	async memberCost(organizationId, memberId, request) {
		await read();
		if (on('viewer'))
			throw fail(
				'not_authorized',
				'Only the owner, and project managers for their projects, see costs.'
			);
		const { snapshot } = await orgApi.view(organizationId);
		const managed = on('manager');
		const allowedIds = managedProjectIds(snapshot);
		const n = await names(organizationId);
		const zone = request.timeZone;
		const allAgent = (on('empty') ? [] : ledgerFor(organizationId)).filter((r) => {
			if (r.consumer.kind !== 'agent' || r.consumer.memberId !== memberId) return false;
			if (managed) return !!(r.attribution.projectId && allowedIds.has(r.attribution.projectId));
			return true;
		});
		const todayP = resolveTodayPeriod(zone);
		const weekP = resolvePeriod({ kind: 'this_week', timeZone: zone }).period;
		const monthP = resolvePeriod({ kind: 'this_month', timeZone: zone }).period;
		const lastMonthP = resolvePeriod({ kind: 'last_month', timeZone: zone }).period;

		// By project (this month + all time? Spec §1: spend by project (with Not on a task) and top tasks by cost)
		// Compute over this month if any records exist, else all recorded (actually consistent across all records in this month, or all-time? Wait: use allAgent in this_month if any, or allAgent? Let's use allAgent in this_month + fall back? No, consistent ledger: use thisMonth for byProject/topTasks or all-time? Wait: let's check: Kai/Sam/Mira/Noor all have records in this_month!).
		// Using allAgent across all periods ensures "top tasks" and "by project" never silently hide recent work while periods show today/thisWeek/thisMonth/lastMonth. Even better: filter by thisMonth? Wait: let's check how many records each agent has in thisMonth vs allTime!
		const projectGroups = new Map<string, StoredRecord[]>();
		const orgOverhead: StoredRecord[] = [];
		for (const r of allAgent) {
			if (!r.attribution.projectId) {
				orgOverhead.push(r);
			} else {
				const key =
					r.attribution.basis === 'project_overhead'
						? `${r.attribution.projectId}:not-on-task`
						: r.attribution.projectId;
				projectGroups.set(key, [...(projectGroups.get(key) ?? []), r]);
			}
		}
		const byProject: MemberProjectSpendRow[] = [...projectGroups.entries()]
			.map(([key, list]) => {
				const isOverhead = key.endsWith(':not-on-task');
				const pid = isOverhead ? key.slice(0, -':not-on-task'.length) : key;
				const pname = n.project.get(pid) ?? pid;
				return {
					id: `member-project:${key}`,
					projectId: pid,
					label: isOverhead ? `${pname} · Not on a task` : pname,
					...(n.project.has(pid) ? {} : { archived: true }),
					...(isOverhead ? { notOnTask: true } : {}),
					totals: totalsOf(list)
				};
			})
			.toSorted(
				(a, b) =>
					Number(!!a.notOnTask) - Number(!!b.notOnTask) ||
					(b.totals.knownCostMicroUsd ?? -1) - (a.totals.knownCostMicroUsd ?? -1) ||
					a.label.localeCompare(b.label)
			);
		if (!managed && orgOverhead.length > 0) {
			byProject.push({
				id: 'member-project:org-not-on-task',
				label: 'Not on a task (organization)',
				notOnTask: true,
				totals: totalsOf(orgOverhead)
			});
		}

		const taskGroups = new Map<string, StoredRecord[]>();
		for (const r of allAgent) {
			if (!r.attribution.taskRef) continue;
			taskGroups.set(r.attribution.taskRef, [...(taskGroups.get(r.attribution.taskRef) ?? []), r]);
		}
		const topTasks: MemberTaskSpendRow[] = [...taskGroups.entries()]
			.map(([ref, list]) => {
				const sample = list[0];
				return {
					taskRef: ref,
					title: sample.attribution.taskTitle ?? ref,
					...(sample.attribution.projectId
						? {
								projectId: sample.attribution.projectId,
								projectName:
									n.project.get(sample.attribution.projectId) ?? sample.attribution.projectId
							}
						: {}),
					totals: totalsOf(list)
				};
			})
			.toSorted(
				(a, b) =>
					(b.totals.knownCostMicroUsd ?? -1) - (a.totals.knownCostMicroUsd ?? -1) ||
					a.taskRef.localeCompare(b.taskRef)
			)
			.slice(0, 10);

		let budgets: MemberCostOverview['budgets'];
		if (!managed) {
			const bZone = on('no-zone') ? null : budgetData.zone.name;
			const fullAgentRecords = on('empty')
				? []
				: ledgerFor(organizationId).filter(
						(r) => r.consumer.kind === 'agent' && r.consumer.memberId === memberId
					);
			const unmeasured = fullAgentRecords.some((r) => r.runtime === 'antigravity');
			const b = budgetsFor(organizationId)[memberId];
			if (bZone && b && !on('empty')) {
				const { day, month } = budgetWindows(bZone);
				budgets = {
					zone: { name: bZone, source: budgetData.zone.source as 'owner_account' | 'hq' },
					...(b.daily ? { daily: budgetState(b.daily, fullAgentRecords, day, unmeasured) } : {}),
					...(b.monthly
						? { monthly: budgetState(b.monthly, fullAgentRecords, month, unmeasured) }
						: {}),
					canManage: true,
					...(on('budget-reset')
						? {
								resetNote:
									'Budget reset: the previous daily period closed and its budget hold was released.'
							}
						: {})
				};
			} else {
				budgets = {
					zone: { name: bZone, source: budgetData.zone.source as 'owner_account' | 'hq' },
					canManage: true
				};
			}
		}

		return structuredClone({
			memberId,
			scope: managed ? ('managed' as const) : ('organization' as const),
			timeZone: zone,
			periods: {
				today: totalsOf(allAgent.filter((r) => within(r, todayP))),
				thisWeek: totalsOf(allAgent.filter((r) => within(r, weekP))),
				thisMonth: totalsOf(allAgent.filter((r) => within(r, monthP))),
				lastMonth: totalsOf(allAgent.filter((r) => within(r, lastMonthP)))
			},
			byProject,
			topTasks,
			...(budgets ? { budgets } : {}),
			coverage: {
				checkpointAt: usage.checkpointAt,
				gaps: coverageGapsFor(organizationId)
			}
		} satisfies MemberCostOverview);
	},

	async memberRecords(organizationId, memberId, request, cursor) {
		await read();
		if (on('viewer'))
			throw fail(
				'not_authorized',
				'Only the owner, and project managers for their projects, see costs.'
			);
		if (on('records-fail'))
			throw fail('internal', "HQ didn't answer. Check that the office is running, then try again.");
		const { snapshot } = await orgApi.view(organizationId);
		const managed = on('manager');
		const allowedIds = managedProjectIds(snapshot);
		const n = await names(organizationId);
		const list = (on('empty') ? [] : ledgerFor(organizationId))
			.filter((r) => {
				if (r.consumer.kind !== 'agent' || r.consumer.memberId !== memberId) return false;
				if (managed && (!r.attribution.projectId || !allowedIds.has(r.attribution.projectId)))
					return false;
				return matchesMemberFilter(r, request.filter, request.timeZone);
			})
			.toSorted((a, b) => b.occurredAt.localeCompare(a.occurredAt));
		const offset = Number(cursor ?? 0);
		const page = list.slice(offset, offset + PAGE).map((r) => toRecord(r, n));
		return structuredClone({
			records: page,
			total: list.length,
			...(offset + PAGE < list.length ? { nextCursor: String(offset + PAGE) } : {})
		});
	},

	async setAgentBudget(organizationId, memberId, draft) {
		await new Promise((r) => setTimeout(r, on('slow') ? 2000 : 250));
		if (on('unavailable')) await unavailableCall();
		if (on('viewer') || on('manager'))
			throw fail('not_authorized', 'Only the organization owner can change agent budgets.');
		if (on('save-fails'))
			throw fail('storage_error', "HQ couldn't write the budget policy. Nothing changed.");
		if (draft.limit <= 0 || !Number.isSafeInteger(draft.limit))
			throw fail('invalid_request', 'Enter a positive budget limit.', { field: 'limit' });
		const zone = on('no-zone') ? null : budgetData.zone.name;
		if (!zone)
			throw fail('invalid_request', "Agent budgets can't run without a known budget time zone.");
		const orgBudgets = budgetsFor(organizationId);
		const entry = (orgBudgets[memberId] ??= {});
		const mine = ledgerFor(organizationId).filter(
			(r) => r.consumer.kind === 'agent' && r.consumer.memberId === memberId
		);
		const unmeasured = mine.some((r) => r.runtime === 'antigravity');
		const { day, month } = budgetWindows(zone);
		const window = draft.periodKind === 'daily' ? day : month;
		const nextStored: StoredBudget = { unit: draft.unit, limit: draft.limit };
		const preview = budgetState(nextStored, mine, window, unmeasured);
		if (preview.spent >= preview.limit) nextStored.hold = 'stop_pending';
		entry[draft.periodKind] = nextStored;
		budgetOperations.set(draft.operationId, { applied: true, message: 'Budget saved.' });
		if (on('save-unknown') && !budgetUnknownOnce) {
			budgetUnknownOnce = true;
			throw Object.assign(new Error('The connection to HQ dropped before it answered.'), {
				source: 'app',
				code: OUTCOME_UNKNOWN,
				message: 'The connection to HQ dropped before it answered.'
			});
		}
		return structuredClone(budgetState(nextStored, mine, window, unmeasured));
	},

	async agentBudgetAction(organizationId, memberId, action) {
		await new Promise((r) => setTimeout(r, on('slow') ? 2000 : 250));
		if (on('unavailable')) await unavailableCall();
		if (on('viewer') || on('manager'))
			throw fail('not_authorized', 'Only the organization owner can manage agent budgets.');
		if (on('save-fails'))
			throw fail('storage_error', "HQ couldn't record the budget change. Nothing changed.");
		const replay = budgetOperations.get(action.operationId);
		if (replay) return { message: replay.message };
		const entry = budgetsFor(organizationId)[memberId];
		const current = entry?.[action.periodKind];
		if (!current) throw fail('not_found', 'No budget is set for this period.');
		let message = 'Budget updated.';
		if (action.action === 'clear') {
			delete entry[action.periodKind];
			message = `${action.periodKind === 'daily' ? 'Daily' : 'Monthly'} budget cleared.`;
		} else if (action.action === 'allow_once') {
			if (current.extended)
				throw fail('invalid_request', 'This budget already used its one 20% extension.');
			const zone = budgetData.zone.name;
			const mine = ledgerFor(organizationId).filter(
				(r) => r.consumer.kind === 'agent' && r.consumer.memberId === memberId
			);
			const { day, month } = budgetWindows(zone);
			const stateNow = budgetState(
				current,
				mine,
				action.periodKind === 'daily' ? day : month,
				false
			);
			if (stateNow.spent < stateNow.limit)
				throw fail(
					'invalid_request',
					'Allow 20% once is available only when the budget is reached.'
				);
			const inc = Math.floor(current.limit / 5);
			if (inc <= 0 || current.limit + inc <= stateNow.spent)
				throw fail(
					'invalid_request',
					'Adding 20% would not exceed current spend. Raise the budget instead.'
				);
			current.extended = true;
			delete current.hold;
			message = 'Allowed 20% more once for this period.';
		} else if (action.action === 'keep_paused') {
			current.hold = 'held';
			message = 'Agent will remain paused until the period resets.';
		}
		budgetOperations.set(action.operationId, { applied: true, message });
		if (on('action-unknown') && !budgetActionUnknownOnce) {
			budgetActionUnknownOnce = true;
			throw Object.assign(new Error('The connection to HQ dropped before it answered.'), {
				source: 'app',
				code: OUTCOME_UNKNOWN,
				message: 'The connection to HQ dropped before it answered.'
			});
		}
		return { message };
	},

	async agentBudgetOperation(_organizationId, operationId) {
		await new Promise((r) => setTimeout(r, 200));
		if (on('unavailable')) await unavailableCall();
		return budgetOperations.has(operationId)
			? { state: 'applied' }
			: { state: 'not_applied', reason: 'HQ has no record of this budget change.' };
	},

	async subscriptionUsage(organizationId) {
		await read();
		ownerOnly();
		if (on('empty')) {
			return structuredClone({
				snapshots: [],
				totalsByModel: [],
				partialTotals: false,
				excludedSnapshotCount: 0,
				isApiEquivalentEstimate: false
			} satisfies SubscriptionUsageOverview);
		}
		const snaps = subscriptionSnapshotsFor(organizationId);
		return structuredClone(buildSubscriptionOverview(snaps));
	},

	async collectSubscriptionUsage(organizationId, options) {
		await read();
		ownerOnly();
		if (on('collect-fails')) {
			throw fail(
				'unavailable',
				'Provider usage endpoint could not be reached. Try refreshing again shortly.'
			);
		}
		const snaps = subscriptionSnapshotsFor(organizationId);
		const nowIso = new Date(NOW.getTime() + 5 * 60_000).toISOString();
		const targetIds = options?.connectionId
			? new Set([options.connectionId])
			: new Set(snaps.map((s) => s.connectionId));
		for (const snap of snaps) {
			if (!targetIds.has(snap.connectionId)) continue;
			if (snap.status === 'auth_required' && !snap.hasUsageCredential) {
				snap.collectedAt = nowIso;
				continue;
			}
			snap.status = 'ok';
			snap.stale = false;
			snap.collectedAt = nowIso;
			snap.lastOkCollectedAt = nowIso;
			snap.freshUntil = new Date(Date.parse(nowIso) + 15 * 60_000).toISOString();
			delete snap.issueCode;
			delete snap.message;
			if (snap.models.length === 0) {
				snap.models = snap.providerType === 'codex' ? defaultCodexModels() : defaultClaudeModels();
			} else {
				for (const m of snap.models) {
					if (m.daily.requests !== null && m.daily.requests !== undefined) {
						m.daily.requests += 2;
					}
					if (m.total.requests !== null && m.total.requests !== undefined) {
						m.total.requests += 2;
					}
				}
			}
		}
		return structuredClone({
			snapshots: snaps.filter((s) => targetIds.has(s.connectionId)),
			collectedAt: nowIso
		});
	},

	async setSubscriptionUsageCredential(organizationId, draft) {
		await read();
		ownerOnly();
		if (!draft.secret.trim()) {
			throw fail('invalid_request', 'A usage credential secret is required.');
		}
		const snaps = subscriptionSnapshotsFor(organizationId);
		const snap = snaps.find((s) => s.connectionId === draft.connectionId);
		if (!snap) throw fail('not_found', 'Subscription connection not found.');
		const nowIso = new Date().toISOString();
		snap.hasUsageCredential = true;
		snap.authSource = 'dedicated_usage_credential';
		if (draft.accountHint) snap.accountHint = draft.accountHint;
		// Mirror crates/org-office/src/accounting/subscription_usage.rs:229 — saving a credential
		// resolves missing auth to `never_collected` until a separate collection run succeeds.
		if (snap.status === 'auth_required' || snap.models.length === 0) {
			snap.status = 'never_collected';
			snap.stale = true;
			snap.issueCode = 'NOT_COLLECTED_YET';
			snap.message = 'Run usage collection to fetch subscription usage from provider.';
		}
		return structuredClone({
			connectionId: snap.connectionId,
			officeId: snap.officeId,
			configured: true,
			configuredAt: nowIso,
			...(draft.expiresAt ? { expiresAt: draft.expiresAt } : {}),
			...(snap.accountHint ? { accountHint: snap.accountHint } : {})
		});
	},

	async revokeSubscriptionUsageCredential(organizationId, connectionId) {
		await read();
		ownerOnly();
		const snaps = subscriptionSnapshotsFor(organizationId);
		const snap = snaps.find((s) => s.connectionId === connectionId);
		if (!snap) throw fail('not_found', 'Subscription connection not found.');
		snap.hasUsageCredential = false;
		if (!snap.hasEnrolledCredential || on('auth-required') || snap.providerType === 'codex') {
			snap.authSource = 'none';
			snap.status = 'auth_required';
			snap.models = [];
			snap.message =
				'Usage credential revoked. Save a new private credential to resume collection.';
		} else {
			snap.authSource = 'enrolled_subscription_credential';
		}
		return structuredClone({
			connectionId: snap.connectionId,
			officeId: snap.officeId,
			configured: false
		});
	}
};

// ---- Subscription Model Usage Fixtures (TASK-79) ------------------------------------------------------------------

function defaultClaudeModels(): SubscriptionModelUsage[] {
	return [
		{
			model: 'claude-opus-4-1',
			total: { utilizationBps: null, requests: 342, totalTokens: 18_400_000 },
			daily: { windowSeconds: 86_400, utilizationBps: 4200, requests: 48, totalTokens: 2_650_000 },
			weekly: {
				windowSeconds: 604_800,
				utilizationBps: 6150,
				requests: 214,
				totalTokens: 11_900_000
			},
			shortTerm: {
				windowSeconds: 18_000,
				utilizationBps: 3850,
				requests: null,
				totalTokens: null
			}
		},
		{
			model: 'claude-sonnet-4-5',
			total: { utilizationBps: null, requests: 910, totalTokens: 42_100_000 },
			daily: { windowSeconds: 86_400, utilizationBps: 2800, requests: 126, totalTokens: 5_800_000 },
			weekly: {
				windowSeconds: 604_800,
				utilizationBps: 4400,
				requests: 580,
				totalTokens: 27_400_000
			},
			shortTerm: {
				windowSeconds: 18_000,
				utilizationBps: 1900,
				requests: 31,
				totalTokens: 1_250_000
			}
		}
	];
}

function defaultCodexModels(): SubscriptionModelUsage[] {
	return [
		{
			model: 'gpt-5-codex',
			total: { utilizationBps: null, requests: 518, totalTokens: 29_300_000 },
			daily: { windowSeconds: 86_400, utilizationBps: 5100, requests: 74, totalTokens: 4_120_000 },
			weekly: {
				windowSeconds: 604_800,
				utilizationBps: 5500,
				requests: 332,
				totalTokens: 19_200_000
			},
			shortTerm: {
				windowSeconds: 18_000,
				utilizationBps: 2400,
				requests: 19,
				totalTokens: null
			}
		}
	];
}

const subscriptionsByOrg = new Map<
	string,
	{ modeKey: string; snapshots: SubscriptionUsageSnapshot[] }
>();

function subscriptionSnapshotsFor(organizationId: string): SubscriptionUsageSnapshot[] {
	const modeKey = `${on('auth-required') ? 'auth' : ''}|${on('stale') ? 'stale' : ''}`;
	const existing = subscriptionsByOrg.get(organizationId);
	if (existing && existing.modeKey === modeKey) return existing.snapshots;

	const authReq = on('auth-required');
	const isStale = on('stale');
	const snapshots: SubscriptionUsageSnapshot[] = [
		{
			connectionId: 'sub-claude-pro',
			connectionKey: 'claude-team-max',
			connectionName: 'Claude Team Max',
			providerType: 'claude',
			subscriptionIdentity: 'org_claude_lumen',
			officeId: 'office-hq',
			status: 'ok',
			authSource: 'enrolled_subscription_credential',
			hasUsageCredential: false,
			hasEnrolledCredential: true,
			accountHint: 'eng@lumen.dev',
			collectedAt: isStale ? '2026-11-19T12:15:00.000Z' : '2026-11-19T14:52:00.000Z',
			lastOkCollectedAt: isStale ? '2026-11-19T12:15:00.000Z' : '2026-11-19T14:52:00.000Z',
			freshUntil: isStale ? '2026-11-19T12:30:00.000Z' : '2026-11-19T15:07:00.000Z',
			stale: isStale,
			...(isStale
				? { message: 'Snapshot is older than the freshness window. Select Refresh to update.' }
				: {}),
			isApiEquivalentEstimate: false,
			models: defaultClaudeModels()
		},
		{
			connectionId: 'sub-codex-team',
			connectionKey: 'codex-pro-workspace',
			connectionName: 'Codex Pro Workspace',
			providerType: 'codex',
			subscriptionIdentity: 'org_codex_lumen',
			officeId: 'office-hq',
			status: authReq ? 'auth_required' : 'ok',
			authSource: authReq ? 'none' : 'dedicated_usage_credential',
			hasUsageCredential: !authReq,
			hasEnrolledCredential: true,
			accountHint: 'platform@lumen.dev',
			collectedAt: '2026-11-19T14:50:00.000Z',
			...(authReq ? {} : { lastOkCollectedAt: '2026-11-19T14:50:00.000Z' }),
			freshUntil: '2026-11-19T15:05:00.000Z',
			stale: false,
			...(authReq
				? {
						issueCode: 'auth_required',
						message:
							'Provider rejected the usage credential. Save a new private usage credential to resume collection.'
					}
				: {}),
			isApiEquivalentEstimate: false,
			models: authReq ? [] : defaultCodexModels()
		}
	];
	subscriptionsByOrg.set(organizationId, { modeKey, snapshots });
	return snapshots;
}

function buildSubscriptionOverview(
	snapshots: SubscriptionUsageSnapshot[]
): SubscriptionUsageOverview {
	const excluded = snapshots.filter((s) => s.status !== 'ok' || s.stale);
	const included = snapshots.filter((s) => s.status === 'ok' && !s.stale);
	const byModel = new Map<string, SubscriptionModelUsage>();
	for (const snap of included) {
		for (const m of snap.models) {
			if (!byModel.has(m.model)) {
				byModel.set(m.model, structuredClone(m));
			}
		}
	}
	const latest = snapshots
		.map((s) => s.collectedAt)
		.filter((v): v is string => Boolean(v))
		.toSorted()
		.at(-1);
	return {
		snapshots,
		totalsByModel: [...byModel.values()].toSorted((a, b) => a.model.localeCompare(b.model)),
		...(latest ? { collectedAsOf: latest } : {}),
		partialTotals: excluded.length > 0,
		excludedSnapshotCount: excluded.length,
		isApiEquivalentEstimate: false
	};
}

// ---- Step 7 Insights & Member Cost Helpers ------------------------------------------------------------------------

function managedProjectIds(snapshot: Awaited<ReturnType<typeof orgApi.view>>['snapshot']) {
	const above = (teamId: string | undefined): string[] => {
		const team = snapshot.teams.find((t) => t.id === teamId);
		return team ? [team.id, ...above(team.owner_team_id)] : [];
	};
	const owner = (projectId: string) =>
		(snapshot.project_teams ?? []).find((l) => l.project_id === projectId)?.team_id;
	return new Set(
		(snapshot.projects ?? []).filter((p) => above(owner(p.id)).includes('web')).map((p) => p.id)
	);
}

function budgetWindows(zone: string) {
	const local = localParts(NOW, zone);
	const nextDay = new Date(Date.UTC(local.y, local.m - 1, local.d + 1));
	const day = {
		key: `${local.y}-${String(local.m).padStart(2, '0')}-${String(local.d).padStart(2, '0')}`,
		startsAt: dayStart(local.y, local.m, local.d, zone),
		endsAt: dayStart(
			nextDay.getUTCFullYear(),
			nextDay.getUTCMonth() + 1,
			nextDay.getUTCDate(),
			zone
		)
	};
	const month = {
		key: `${local.y}-${String(local.m).padStart(2, '0')}`,
		startsAt: dayStart(local.y, local.m, 1, zone),
		endsAt: dayStart(
			local.m === 12 ? local.y + 1 : local.y,
			local.m === 12 ? 1 : local.m + 1,
			1,
			zone
		)
	};
	return { day, month };
}

function resolveTodayPeriod(zone: string): ResolvedPeriod {
	const local = localParts(NOW, zone);
	const start = dayStart(local.y, local.m, local.d, zone);
	return {
		kind: 'custom',
		startsAt: start.toISOString(),
		endsAt: NOW.toISOString(),
		timeZone: zone,
		toNow: true
	};
}

function isCancelledTask(r: StoredRecord): boolean {
	return r.attribution.taskRef === 'T-21' || r.attribution.taskRef === 'T-40';
}

function taskTypeOf(r: StoredRecord): string {
	if (!r.attribution.taskRef) return 'not_on_task';
	const title = (r.attribution.taskTitle ?? '').toLowerCase();
	if (title.includes('fix') || title.includes('bug') || title.includes('error')) return 'bug';
	if (title.includes('redesign') || title.includes('request')) return 'request';
	return 'task';
}

const TASK_TYPE_LABELS: Record<string, string> = {
	task: 'Task',
	bug: 'Bug',
	request: 'Request',
	not_on_task: 'Not on a task'
};

function countWorkDone(records: StoredRecord[], projectDone: number, kind: PeriodRequest['kind']) {
	const distinctTasks = new Set(records.map((r) => r.attribution.taskRef).filter(Boolean));
	if (records.length === 0) return { tasksDone: 0, bugsDone: 0 };
	const base = kind === 'all_time' ? Math.max(projectDone, distinctTasks.size) : distinctTasks.size;
	const bugsDone = Math.max(1, Math.floor(base / 3));
	const tasksDone = Math.max(1, base - bugsDone);
	return { tasksDone, bugsDone };
}

function matchesProjectKey(r: StoredRecord, key: ProjectBreakdownKey): boolean {
	if ('all' in key) return true;
	switch (key.dimension) {
		case 'team':
			return 'notOnTask' in key
				? r.attribution.basis === 'project_overhead' || !r.attribution.taskRef
				: r.attribution.teamId === key.teamId && !!r.attribution.taskRef;
		case 'consumer':
			return 'oneOff' in key
				? r.consumer.kind === 'one_off'
				: r.consumer.kind === 'agent' && r.consumer.memberId === key.memberId;
		case 'model':
			return r.provider === key.provider && r.model === key.model;
		case 'task_type':
			return taskTypeOf(r) === key.taskType;
		case 'work_type':
			return key.workType === 'not_recorded' ? r.workType === null : r.workType === key.workType;
	}
}

function matchesMemberFilter(r: StoredRecord, filter: MemberRecordsFilter, zone: string): boolean {
	switch (filter.kind) {
		case 'all':
			return true;
		case 'period': {
			const p =
				filter.period === 'today'
					? resolveTodayPeriod(zone)
					: resolvePeriod({ kind: filter.period, timeZone: zone }).period;
			return within(r, p);
		}
		case 'project':
			return (
				r.attribution.projectId === filter.projectId &&
				(filter.notOnTask
					? r.attribution.basis === 'project_overhead'
					: r.attribution.basis !== 'project_overhead')
			);
		case 'org_not_on_task':
			return !r.attribution.projectId;
		case 'task':
			return r.attribution.taskRef === filter.taskRef;
	}
}

function buildTrend(records: StoredRecord[], period: ResolvedPeriod): TrendBucket[] {
	if (records.length === 0) return [];
	const zone = period.timeZone;
	const endDate = new Date(period.endsAt);
	const earliest = records.reduce(
		(acc, r) => (r.occurredAt < acc ? r.occurredAt : acc),
		records[0].occurredAt
	);
	const startDate = period.startsAt ? new Date(period.startsAt) : new Date(earliest);
	const spanDays = Math.max(1, Math.ceil((endDate.getTime() - startDate.getTime()) / 86_400_000));
	const useWeeks = spanDays > 31;
	const fmtShort = new Intl.DateTimeFormat('en-GB', {
		day: 'numeric',
		month: 'short',
		timeZone: zone
	});

	const buckets: TrendBucket[] = [];
	let cursor = new Date(startDate);
	let safety = 0;
	while (cursor < endDate && safety < 60) {
		safety++;
		const p = localParts(cursor, zone);
		let next: Date;
		if (useWeeks) {
			const back = WEEKDAYS.indexOf(p.weekday);
			const nextMon = new Date(Date.UTC(p.y, p.m - 1, p.d - back + 7));
			next = dayStart(
				nextMon.getUTCFullYear(),
				nextMon.getUTCMonth() + 1,
				nextMon.getUTCDate(),
				zone
			);
		} else {
			const nextDay = new Date(Date.UTC(p.y, p.m - 1, p.d + 1));
			next = dayStart(
				nextDay.getUTCFullYear(),
				nextDay.getUTCMonth() + 1,
				nextDay.getUTCDate(),
				zone
			);
		}
		const bucketEnd = next > endDate ? endDate : next;
		const startsAt = cursor.toISOString();
		const endsAt = bucketEnd.toISOString();
		const inBucket = records.filter((r) => r.occurredAt >= startsAt && r.occurredAt < endsAt);
		const partial =
			(safety === 1 && useWeeks && WEEKDAYS.indexOf(p.weekday) > 0) ||
			(bucketEnd.getTime() === endDate.getTime() && period.toNow);
		buckets.push({
			key: startsAt.slice(0, 10),
			label: useWeeks ? `Wk ${fmtShort.format(cursor)}` : fmtShort.format(cursor),
			startsAt,
			endsAt,
			...(partial ? { partial: true } : {}),
			totals: totalsOf(inBucket)
		});
		cursor = bucketEnd;
	}
	return buckets;
}

function buildProjectBreakdowns(
	records: StoredRecord[],
	n: Awaited<ReturnType<typeof names>>
): ProjectInsights['breakdowns'] {
	const makeRows = <K extends string>(
		list: StoredRecord[],
		keyOf: (r: StoredRecord) => K,
		build: (
			key: K,
			items: StoredRecord[]
		) => Omit<
			InsightBreakdownRow,
			| 'totals'
			| 'itemsDone'
			| 'costPerTaskMicroUsd'
			| 'medianLeadSeconds'
			| 'medianCycleSeconds'
			| 'sampleCount'
		> & { overhead?: boolean }
	): InsightBreakdownRow[] => {
		const groups = new Map<K, StoredRecord[]>();
		for (const r of list) groups.set(keyOf(r), [...(groups.get(keyOf(r)) ?? []), r]);
		return [...groups.entries()].map(([k, items]) => {
			const base = build(k, items);
			const t = totalsOf(items);
			const taskSet = new Set(items.map((r) => r.attribution.taskRef).filter(Boolean));
			const itemsDone = base.overhead ? null : taskSet.size;
			const costPerTaskMicroUsd =
				itemsDone && itemsDone > 0 && t.knownCostMicroUsd !== null
					? Math.round(t.knownCostMicroUsd / itemsDone)
					: null;
			const sampleCount = itemsDone ?? 0;
			const medianLeadSeconds = sampleCount >= 5 ? 95_040 + (k.length % 4) * 18_000 : null;
			const medianCycleSeconds = sampleCount >= 5 ? 43_200 + (k.length % 3) * 10_800 : null;
			return {
				...base,
				totals: t,
				itemsDone,
				costPerTaskMicroUsd,
				medianLeadSeconds,
				medianCycleSeconds,
				sampleCount
			};
		});
	};

	const byTeam = makeRows(
		records,
		(r) =>
			r.attribution.basis === 'project_overhead' || !r.attribution.taskRef
				? ':not-on-task'
				: (r.attribution.teamId ?? ':not-on-task'),
		(k) =>
			k === ':not-on-task'
				? {
						id: 'team:not-on-task',
						key: { dimension: 'team', notOnTask: true },
						label: 'Not on a task',
						overhead: true
					}
				: {
						id: `team:${k}`,
						key: { dimension: 'team', teamId: k },
						label: n.team.get(k) ?? k,
						teamId: k
					}
	);

	const byConsumer = makeRows(
		records,
		(r) => (r.consumer.kind === 'agent' ? r.consumer.memberId : ':one-off'),
		(k) =>
			k === ':one-off'
				? {
						id: 'consumer:one-off',
						key: { dimension: 'consumer', oneOff: true },
						label: 'One-off model calls (drafting)'
					}
				: {
						id: `consumer:${k}`,
						key: { dimension: 'consumer', memberId: k },
						label: n.member.get(k) ?? k,
						memberId: k
					}
	);

	const byModel = makeRows(
		records,
		(r) => `${r.provider ?? 'unknown'}/${r.model ?? 'unknown'}`,
		(k, items) => ({
			id: `model:${k}`,
			key: { dimension: 'model', provider: items[0].provider, model: items[0].model },
			label: items[0].model ?? 'Unknown model',
			subtitle: items[0].provider ?? undefined
		})
	);

	const byTaskType = makeRows(
		records,
		(r) => taskTypeOf(r),
		(k) => ({
			id: `task-type:${k}`,
			key: { dimension: 'task_type', taskType: k },
			label: TASK_TYPE_LABELS[k] ?? k,
			...(k === 'not_on_task' ? { overhead: true } : {})
		})
	);

	const WORK_LABELS = { build: 'Build', run: 'Run', not_recorded: 'Not recorded' } as const;
	const byWorkType = makeRows(
		records,
		(r) => r.workType ?? 'not_recorded',
		(k) => ({
			id: `work-type:${k}`,
			key: { dimension: 'work_type', workType: k },
			label: WORK_LABELS[k]
		})
	);

	return { byTeam, byConsumer, byModel, byTaskType, byWorkType };
}

function buildProjectInsightsNotices(
	projectId: string,
	projectName: string,
	records: StoredRecord[],
	breakdowns: ProjectInsights['breakdowns']
): InsightNotice[] {
	if (records.length === 0) return [];
	const notices: InsightNotice[] = [];
	const taskTotals = new Map<string, { title: string; cost: number }>();
	for (const r of records) {
		if (!r.attribution.taskRef || r.costMicroUsd === null) continue;
		const cur = taskTotals.get(r.attribution.taskRef) ?? {
			title: r.attribution.taskTitle ?? r.attribution.taskRef,
			cost: 0
		};
		cur.cost += r.costMicroUsd;
		taskTotals.set(r.attribution.taskRef, cur);
	}
	const sortedTasks = [...taskTotals.entries()].toSorted((a, b) => b[1].cost - a[1].cost);
	if (sortedTasks.length >= 2) {
		const [topRef, top] = sortedTasks[0];
		const costs = sortedTasks.map(([, v]) => v.cost).toSorted((a, b) => a - b);
		const median = costs[Math.floor(costs.length / 2)];
		if (median > 0 && top.cost >= median * 2) {
			const mult = (top.cost / median).toFixed(1).replace(/\.0$/, '');
			notices.push({
				id: `outlier-${topRef}`,
				kind: 'outlier',
				tone: 'owner',
				text: `${topRef} "${top.title}" cost ${mult}× the median item in ${projectName} (≈ $${(top.cost / 1_000_000).toFixed(2)} vs ≈ $${(median / 1_000_000).toFixed(2)})`,
				actionLabel: 'Open',
				href: `/work/${encodeURIComponent(topRef)}`,
				limitation: `Based on ${sortedTasks.length} tasks with known cost in this period.`
			});
		}
	}
	notices.push({
		id: `waiting-${projectId}`,
		kind: 'waiting',
		tone: 'neutral',
		text: `Agents waited ${projectId === 'beacon' ? '19 h' : '11 h'} for your answers in this period.`,
		actionLabel: 'See questions',
		href: '/inbox?tab=questions',
		limitation: 'Counts recorded owner questions and approvals; working time is not tracked yet.'
	});
	const totalKnown = totalsOf(records).knownCostMicroUsd ?? 0;
	const topTeam = breakdowns.byTeam
		.filter((r) => !r.overhead && r.totals.knownCostMicroUsd !== null)
		.toSorted((a, b) => (b.totals.knownCostMicroUsd ?? 0) - (a.totals.knownCostMicroUsd ?? 0))[0];
	if (topTeam && totalKnown > 0) {
		const share = Math.round(((topTeam.totals.knownCostMicroUsd ?? 0) * 100) / totalKnown);
		if (share >= 40) {
			notices.push({
				id: `concentration-${topTeam.id}`,
				kind: 'concentration',
				tone: 'neutral',
				text: `${topTeam.label} accounts for ${share}% of ${projectName}'s known cost in this period.`,
				actionLabel: 'View team',
				href: topTeam.teamId
					? `/teams/${encodeURIComponent(topTeam.teamId)}`
					: `/projects/${encodeURIComponent(projectId)}/team`,
				limitation: 'Share of known dollars only; unpriced and unknown records are excluded.'
			});
		}
	}
	return notices;
}

function buildOrgInsightsNotices(
	projects: OrgInsightProjectRow[],
	totals: UsageTotals
): InsightNotice[] {
	if (totals.records === 0) return [];
	const notices: InsightNotice[] = [];
	const totalKnown = totals.knownCostMicroUsd ?? 0;
	const top = projects
		.filter((p) => p.periodTotals.knownCostMicroUsd !== null)
		.toSorted(
			(a, b) => (b.periodTotals.knownCostMicroUsd ?? 0) - (a.periodTotals.knownCostMicroUsd ?? 0)
		)[0];
	if (top && totalKnown > 0) {
		const share = Math.round(((top.periodTotals.knownCostMicroUsd ?? 0) * 100) / totalKnown);
		if (share >= 40) {
			notices.push({
				id: `org-concentration-${top.projectId}`,
				kind: 'concentration',
				tone: 'neutral',
				text: `${top.name} has ${share}% of known cost in this period.`,
				actionLabel: 'Open insights',
				href: `/projects/${encodeURIComponent(top.projectId)}/insights`,
				limitation: 'Share of known cost across visible projects.'
			});
		}
	}
	notices.push({
		id: 'org-waiting',
		kind: 'waiting',
		tone: 'owner',
		text: 'Agents waited 30 h for your answers in this period.',
		actionLabel: 'See questions',
		href: '/inbox?tab=questions',
		limitation: 'Union of recorded owner waits; working time is not tracked yet.'
	});
	return notices;
}

function buildReleaseCaps(projectId: string, projectRecords: StoredRecord[]): ReleaseCapRow[] {
	const entry = releaseMeta[projectId];
	if (!entry || entry.releases.length === 0) return [];
	const totalKnown = totalsOf(projectRecords).knownCostMicroUsd ?? 0;
	const complete = totalsOf(projectRecords).costIncompleteRecords === 0;
	return entry.releases.map((rel, i) => {
		const spent = i === 0 ? Math.round(totalKnown * 0.65) : Math.round(totalKnown * 0.35);
		const capLimit = i === 0 ? 50_000_000 : 120_000_000;
		const isReleased = rel.status === 'released';
		return {
			releaseId: rel.id,
			number: rel.number,
			title: rel.title,
			...(rel.versionLabel ? { versionLabel: rel.versionLabel } : {}),
			unit: 'usd',
			capLimit,
			spent,
			complete,
			...(isReleased
				? {
						releasedAt: rel.updatedAt ?? '2026-11-14T11:15:00Z',
						spentAtRelease: Math.max(0, spent - 1_400_000)
					}
				: {})
		};
	});
}

function buildSpeedRows(records: StoredRecord[]): SpeedKindRow[] {
	if (records.length === 0) return [];
	const byKind = new Map<string, Set<string>>();
	for (const r of records) {
		if (!r.attribution.taskRef) continue;
		const k = taskTypeOf(r);
		const set = byKind.get(k) ?? new Set<string>();
		set.add(r.attribution.taskRef);
		byKind.set(k, set);
	}
	return (['task', 'bug', 'request'] as const)
		.filter((k) => (byKind.get(k)?.size ?? 0) > 0)
		.map((k) => {
			const count = byKind.get(k)!.size;
			return {
				kind: k,
				label: TASK_TYPE_LABELS[k],
				sampleCount: count,
				medianLeadSeconds: count >= 5 ? (k === 'task' ? 138_240 : 64_800) : null,
				medianCycleSeconds: count >= 5 ? (k === 'task' ? 68_400 : 28_800) : null,
				medianWaitingSeconds: count >= 5 ? 10_800 : null
			};
		});
}
