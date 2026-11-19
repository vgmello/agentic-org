// Mock for src/lib/modules/settings/api.ts under `bun run dev:mock`. People, agents and projects come from the org mock so
// the ids match the other pages; who has an account comes from the members mock (signInOf), so General, Members and
// Settings › Members and access agree; everything else is JSON. Mutations change the answers in memory.
// Scenarios (query string): ?recovery=preparing|waiting|shown|shown_again|collected|collected_again|failed|unknown,
// ?admin=other (the viewer is the owner but not the administrator), ?settings=fails|slow|empty.
import { officeApi } from '$lib/office/api';
import { orgApi } from '$lib/org/api';
import type { SettingsApi } from '../../../src/lib/modules/settings/api';
import type {
	CostPeriod,
	GeneralSettings,
	PriceRate,
	RecoveryState
} from '../../../src/lib/modules/settings/types';
import general from '../data/settings/general.json' with { type: 'json' };
import prices0 from '../data/settings/prices.json' with { type: 'json' };
import costs from '../data/settings/costs.json' with { type: 'json' };
import { signInOf } from './members';

const query = () => new URLSearchParams(location.search);
const wait = () =>
	new Promise((r) => setTimeout(r, query().get('settings') === 'slow' ? 2500 : 250));
async function gate() {
	await wait();
	if (query().get('settings') === 'fails')
		throw {
			source: 'app',
			code: 'unreachable',
			message: "HQ didn't answer. Check that it's running, then try again."
		};
}

let administrator = { memberId: 'you', username: general.administrator.username };
/** Usernames of accounts tied to people here; whether the account still exists is `signInOf` (the members mock). */
const accounts: Record<string, string> = {
	...general.accounts,
	you: general.administrator.username
};

/** The instance's administrator account (handed over in General); Settings › Members and access reads it too. */
export const administratorAccount = () => ({ ...administrator });
/** The username of a person's account, when they have one. */
export const usernameOf = (memberId: string): string | undefined =>
	signInOf(memberId).account ? accounts[memberId] : undefined;
let prices: PriceRate[] = structuredClone(prices0) as PriceRate[];

async function snapshot(organizationId: string) {
	return (await orgApi.view(organizationId)).snapshot;
}

async function general_(organizationId: string): Promise<GeneralSettings> {
	const { members } = await snapshot(organizationId);
	const org = (await officeApi.connections()).find(
		(c) => c.organization.id === organizationId
	)?.organization;
	const other = query().get('admin') === 'other';
	const admin = other ? { memberId: 'avery', username: 'avery' } : administrator;
	const isAdministrator = admin.memberId === 'you';
	const person = members.find((m) => m.id === admin.memberId);
	const recovery = (general.recoveries as Record<string, RecoveryState>)[
		query().get('recovery') ?? 'waiting'
	] ?? { state: 'waiting' };
	return {
		organization: { id: organizationId, name: org?.name ?? 'Organization' },
		workspace: general.workspace.replace('Lumen Labs', org?.name ?? 'Organization'),
		hqMachine: general.hqMachine,
		instance: general.instance,
		runtimes: general.runtimes as GeneralSettings['runtimes'],
		viewer: { isOwner: true, isAdministrator },
		recovery,
		administrator: {
			username: admin.username,
			...(person ? { person: person.name } : {}),
			isYou: isAdministrator
		},
		handOverCandidates: members
			.filter((m) => m.kind === 'person' && usernameOf(m.id) && m.id !== administrator.memberId)
			.map((m) => ({ memberId: m.id, name: m.name, username: accounts[m.id] }))
	};
}

const micro = (n: number | null | undefined) => (n === undefined ? null : n);

export const settingsApi: SettingsApi = {
	async general(organizationId) {
		await gate();
		return general_(organizationId);
	},
	async handOverAdministrator(organizationId, memberId, password) {
		await wait();
		if (password === 'wrong')
			throw {
				source: 'app',
				code: 'wrong_password',
				message: "That password doesn't match. Try again."
			};
		if (!usernameOf(memberId))
			throw {
				source: 'app',
				code: 'not_found',
				message: "That person doesn't have an account any more."
			};
		administrator = { memberId, username: accounts[memberId]! };
		return general_(organizationId);
	},
	async prices() {
		await gate();
		return query().get('settings') === 'empty' ? [] : structuredClone(prices);
	},
	async addPrice(_organizationId, rate) {
		await wait();
		const today = new Date().toISOString().slice(0, 10);
		if (rate.effectiveFrom < today)
			throw {
				source: 'app',
				code: 'invalid_date',
				message: 'A new rate takes effect from today or a later date.',
				field: 'effectiveFrom'
			};
		if (
			prices.some(
				(p) =>
					p.model.toLowerCase() === rate.model.toLowerCase() &&
					p.provider.toLowerCase() === rate.provider.toLowerCase() &&
					p.effectiveFrom === rate.effectiveFrom
			)
		)
			throw {
				source: 'app',
				code: 'already_exists',
				message: `There is already a rate for ${rate.model} from that date. Choose another date.`,
				field: 'effectiveFrom'
			};
		prices = [...prices, { ...rate, id: `01JC0PRICE${String(Date.now()).padStart(16, '0')}` }];
		return structuredClone(prices);
	},
	async costSummary(organizationId, period: CostPeriod) {
		await gate();
		const { members, projects } = await snapshot(organizationId);
		if (query().get('admin') === 'viewer')
			throw {
				source: 'app',
				code: 'not_authorized',
				message: 'Only the organization’s owner sees its costs.'
			};
		const p = costs.periods[period];
		const byProject = (projects ?? [])
			.filter((x) => !x.archived)
			.map((x) => ({
				label: x.name,
				microUsd: micro((p.projects as Record<string, number | null>)[x.id])
			}));
		const known = byProject.reduce((sum, x) => sum + (x.microUsd ?? 0), 0) + p.notOnTask;
		return {
			period,
			totalMicroUsd: known,
			includesApiEquivalent: true,
			excluded: p.excluded,
			byProject,
			byConsumer: costs.consumers.map((c) => ({
				label: c.label,
				workType: c.workType as 'Build' | 'Run' | 'Unknown',
				microUsd: c[period],
				...(c.apiEquivalent ? { apiEquivalent: true } : {})
			})),
			notOnTaskMicroUsd: p.notOnTask,
			budgets: costs.budgets.map((b) => ({
				memberId: b.memberId,
				name: members.find((m) => m.id === b.memberId)?.name ?? b.memberId,
				...(b.daily
					? {
							daily: {
								label: `Today (${costs.budgetZone})`,
								spentMicroUsd: b.daily.spent,
								limitMicroUsd: b.daily.limit
							}
						}
					: {}),
				...(b.monthly
					? {
							monthly: {
								label: `This month (${costs.budgetZone})`,
								spentMicroUsd: b.monthly.spent,
								limitMicroUsd: b.monthly.limit
							}
						}
					: {})
			})),
			budgetZone: costs.budgetZone
		};
	}
};
