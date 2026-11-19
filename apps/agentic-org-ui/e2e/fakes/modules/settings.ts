// Mock for src/lib/modules/settings/api.ts under `bun run dev:mock`. People, agents and projects come from the org mock so
// the ids match the other pages; everything else is JSON. Mutations change the answers in memory.
// Scenarios (query string): ?recovery=preparing|waiting|shown|shown_again|collected|collected_again|failed|unknown,
// ?admin=other (the viewer is the owner but not the administrator), ?settings=fails|slow|empty.
import { officeApi } from '$lib/office/api';
import { orgApi } from '$lib/org/api';
import type { SettingsApi } from '../../../src/lib/modules/settings/api';
import type {
	AccessOverview,
	AccessPerson,
	GeneralSettings,
	RecoveryState
} from '../../../src/lib/modules/settings/types';
import general from '../data/settings/general.json' with { type: 'json' };

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
const accounts: Record<string, string> = {
	...general.accounts,
	you: general.administrator.username
};
const links = new Map<string, string>();

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
			.filter((m) => m.kind === 'person' && accounts[m.id] && m.id !== administrator.memberId)
			.map((m) => ({ memberId: m.id, name: m.name, username: accounts[m.id] }))
	};
}

async function access(organizationId: string): Promise<AccessOverview> {
	const { members } = await snapshot(organizationId);
	const people: AccessPerson[] = members
		.filter((m) => m.kind === 'person')
		.map((m) => ({
			memberId: m.id,
			name: m.name,
			...(m.contact ? { title: m.contact } : {}),
			...(accounts[m.id] ? { username: accounts[m.id] } : {}),
			owner: !!m.owner,
			administrator: administrator.memberId === m.id
		}));
	return { people, viewerCanManage: query().get('admin') !== 'viewer' };
}

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
		if (!accounts[memberId])
			throw {
				source: 'app',
				code: 'not_found',
				message: "That person doesn't have an account any more."
			};
		administrator = { memberId, username: accounts[memberId] };
		return general_(organizationId);
	},
	async access(organizationId) {
		await gate();
		return access(organizationId);
	},
	async issueLink(organizationId, memberId, kind) {
		await wait();
		const secret =
			Math.random().toString(36).slice(2, 14) + Math.random().toString(36).slice(2, 10);
		const url = `http://127.0.0.1:7420/${kind === 'invite' ? 'register' : 'set-password'}#${secret}`;
		links.set(memberId, url);
		return { url, kind, expiresAt: new Date(Date.now() + 24 * 3600_000).toISOString() };
	},
	async removeAccess(organizationId, memberId) {
		await wait();
		delete accounts[memberId];
		return access(organizationId);
	}
};
