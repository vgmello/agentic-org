// Mock for src/lib/modules/costs/api.ts under `bun run dev:mock`. One list of usage records (JSON in
// e2e/fakes/data/costs) is the only source: every total, breakdown, budget and drill-down is computed from it, so the
// figures always add up (COST-4). The clock is fixed at the data's `now`, so periods and budgets don't drift; prices
// use the real date, so "today or later" means today. Names of agents, projects and teams come from the org mock, so
// IDs match the other pages. Added rates are kept in memory until the page reloads.
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (reads fail) · empty (no records, no prices) · unavailable · viewer (not the owner)
//   no-zone (the budget zone isn't known) · save-fails (adding a rate fails, verified) · save-unknown (the first add's
//   outcome is unknown; it was in fact applied) · records-fail (the records sheet fails to load)
import { orgApi } from '$lib/org/api';
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
	PeriodRequest,
	PriceVersion,
	ResolvedPeriod,
	UsageRecord,
	UsageTotals
} from '../../../src/lib/modules/costs/types';
import { mockMode } from '../mock-mode';
import usage from '../data/costs/usage.json' with { type: 'json' };
import priceData from '../data/costs/prices.json' with { type: 'json' };
import budgetData from '../data/costs/budgets.json' with { type: 'json' };

type StoredRecord = {
	id: string;
	occurredAt: string;
	consumer: { kind: 'agent'; memberId: string } | { kind: 'one_off'; label: string };
	runtime: string;
	provider: string;
	model: string;
	attribution: {
		basis: UsageRecord['attribution']['basis'];
		taskRef?: string;
		taskTitle?: string;
		projectId?: string;
		teamId?: string;
	};
	workType: 'build' | 'run' | null;
	tokens: [number, number, number, number, number] | null;
	tokenStatus: UsageRecord['tokenStatus'];
	costMicroUsd: number | null;
	costBasis: UsageRecord['costBasis'];
	costStatus: UsageRecord['costStatus'];
	priceId?: string | null;
	apiEquivalent?: boolean;
	issue?: string;
	supersedesId?: string;
	correction?: { reason: string; byName: string; at: string };
};
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

const on = (name: string) => mockMode().split(',').includes(name);
const fail = (code: string, message: string, extra: Record<string, unknown> = {}) =>
	Object.assign(new Error(message), { source: 'api', code, message, ...extra });

async function read() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2500 : 200));
	if (on('unavailable')) await unavailableCall();
	if (on('fails'))
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
		includesApiEquivalent: false
	};
	for (const r of records) {
		if (r.costMicroUsd !== null) cost = (cost ?? 0) + r.costMicroUsd;
		else t.unpricedRecords++;
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
	const spent = stored.unit === 'usd' ? (totals.knownCostMicroUsd ?? 0) : (totals.knownTokens ?? 0);
	const complete =
		stored.unit === 'usd'
			? totals.costIncompleteRecords === 0
			: totals.tokenIncompleteRecords === 0;
	return {
		unit: stored.unit,
		limit: stored.limit,
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
		...(stored.hold && spent >= stored.limit ? { hold: stored.hold } : {}),
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
		await new Promise((r) => setTimeout(r, on('slow') ? 2500 : 500));
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
		await new Promise((r) => setTimeout(r, on('slow') ? 2500 : 300));
		if (on('unavailable')) await unavailableCall();
		const price = operations.get(operationId);
		return price
			? { state: 'applied', price: structuredClone(price) }
			: { state: 'not_applied', reason: 'HQ has no record of this rate.' };
	},

	async organizationCost(organizationId, request) {
		await read();
		ownerOnly();
		const { period, previous } = resolvePeriod(request);
		const n = await names(organizationId);
		const records = on('empty') ? [] : effective.filter((r) => within(r, period));
		const totalsPrevious = previous
			? totalsOf(on('empty') ? [] : effective.filter((r) => within(r, previous)))
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
				gaps: period.toNow && !on('empty') ? usage.gaps : []
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
		const list = (on('empty') ? [] : effective)
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
		const stored = budgetData.budgets as Record<
			string,
			{ daily?: StoredBudget; monthly?: StoredBudget }
		>;
		const agents: AgentBudget[] = n.agents
			.map((m) => {
				const mine = effective.filter(
					(r) => r.consumer.kind === 'agent' && r.consumer.memberId === m.id
				);
				const unmeasured = mine.some((r) => r.runtime === 'antigravity');
				const b = stored[m.id];
				if (!zone || !b || on('empty')) return { memberId: m.id, name: m.name };
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
	}
};
