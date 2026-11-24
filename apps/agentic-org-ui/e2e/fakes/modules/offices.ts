// Mock for src/lib/modules/offices/api.ts under `bun run dev:mock`: the offices beyond the connected one, layered beside the
// office mock's own list. Ids of agents come from the org mock. Mutations change the answers in memory.
// Scenarios, read when a call is made, from `?mock=` (shared with the other fakes) or `?offices=` (this fake only; use it
// beside the office mock's own `?mock=unreachable`), several joined with commas:
//   slow · fails (every call fails) · empty (HQ alone: nothing joined, pending or queued) · unavailable (the production
//   answer) · gone (approving or renewing finds the request or invitation gone)
// and ?admin=other (the owner is not the administrator), ?add=waiting (adding on this instance waits for agents),
// ?add=fails (it fails once), ?add=check-fails (one progress check fails; the add itself goes on). HQ refusing On this instance for a non-administrator is ?admin=other.
import type { OfficeInfo } from '$lib/proto';
import type { OfficesApi } from '../../../src/lib/modules/offices/api';
import type {
	AddProgress,
	Invitation,
	JoinRequest,
	OfficesOverview,
	OutboxItem,
	PendingAdd,
	SatelliteDetails
} from '../../../src/lib/modules/offices/types';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { POC_ORG_ID } from '../mock-mode';
import { pocOfficesOverview } from '../poc-dataset';
import full from '../data/offices/seed.json' with { type: 'json' };

/** HQ alone: the seed with nothing joined, invited, pending or queued. */
const seedFor = (): typeof full =>
	on('empty') ? { ...full, satellites: [], joinRequests: [], invitations: [], outbox: [] } : full;

const query = () => new URLSearchParams(location.search);
const switches = () =>
	[query().get('mock'), query().get('offices')].flatMap((v) => (v ? v.split(',') : []));
const on = (name: string) => switches().includes(name);
const wait = () => (on('slow') ? new Promise<void>((r) => setTimeout(r, 2500)) : Promise.resolve());
const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();
const ahead = (minutes: number) => new Date(Date.now() + minutes * 60_000).toISOString();

interface Op {
	id: string;
	name: string;
	startedAt: number;
	waiting: boolean;
	cancelled: boolean;
	office?: OfficeInfo;
	failed?: boolean;
}

let state:
	| {
			orgId: string;
			empty: boolean;
			satellites: OfficeInfo[];
			details: Record<string, SatelliteDetails>;
			joinRequests: JoinRequest[];
			invitations: Invitation[];
			outbox: OutboxItem[];
			placements: Record<string, string | null>;
			names: Record<string, string>;
			ops: Op[];
			failedOnce: boolean;
			wrongCodes: Record<string, number>;
	  }
	| undefined;

async function ensure(organizationId: string) {
	if (state?.orgId === organizationId && state.empty === on('empty')) return state;
	const seed = seedFor();
	const sats: OfficeInfo[] = [];
	const details: Record<string, SatelliteDetails> = {};
	const placements: Record<string, string | null> = {};
	for (const s of seed.satellites) {
		const id = `office-${organizationId}-${s.key}`;
		sats.push({
			id,
			organization_id: organizationId,
			name: s.name,
			role: 'satellite',
			host: s.host,
			status: s.status as OfficeInfo['status']
		});
		details[id] = {
			officeId: id,
			where: s.where,
			lastSeenAt: ago(s.lastSeenMinutesAgo),
			sync: s.sync as SatelliteDetails['sync'],
			version: s.version,
			follows: s.follows,
			tasks: s.tasks
		};
		for (const agent of s.agents) placements[agent] = id;
	}
	const idOf = (key: string) => `office-${organizationId}-${key}`;
	state = {
		orgId: organizationId,
		empty: on('empty'),
		satellites: sats,
		details,
		joinRequests: seed.joinRequests.map((j) => ({
			id: j.id,
			officeName: j.officeName,
			fromAddress: j.fromAddress,
			requestedAt: ago(j.requestedMinutesAgo),
			invitationCreatedAt: ago(j.invitationCreatedMinutesAgo)
		})),
		invitations: seed.invitations.map((i) => ({
			id: i.id,
			officeName: i.officeName,
			state: i.state as Invitation['state'],
			createdAt: ago(i.createdMinutesAgo),
			expiresAt: ahead(i.expiresInMinutes),
			...('note' in i && i.note ? { note: i.note } : {})
		})),
		outbox: seed.outbox.map((o) => ({
			id: o.id,
			officeId: idOf(o.officeKey),
			officeName: seed.satellites.find((s) => s.key === o.officeKey)?.name ?? o.officeKey,
			description: o.description,
			state: o.state as OutboxItem['state'],
			...('reason' in o && o.reason ? { reason: o.reason } : {}),
			since: ago(o.sinceMinutesAgo)
		})),
		placements,
		names: {},
		ops: [],
		failedOnce: false,
		wrongCodes: {}
	};
	return state;
}

/** `?add=check-fails`: whether the one failing progress check has happened. */
let checkFailed = false;

const secret = () =>
	`ao-invite-${Math.random().toString(36).slice(2, 10)}-${Math.random().toString(36).slice(2, 12)}-${Math.random().toString(36).slice(2, 12)}`;
const inviteTtl = 24 * 60;

async function gate(organizationId: string) {
	await wait();
	if (on('unavailable')) await unavailableCall();
	if (on('fails'))
		throw {
			source: 'app',
			code: 'unreachable',
			message: "HQ didn't answer. Check that it's running, then try again."
		};
	return ensure(organizationId);
}

const nameTaken = (s: NonNullable<typeof state>, name: string) => {
	const n = name.trim().toLowerCase();
	return (
		s.satellites.some((o) => (s.names[o.id] ?? o.name).toLowerCase() === n) ||
		s.invitations.some((i) => i.state === 'invited' && i.officeName.toLowerCase() === n) ||
		n === 'hq'
	);
};

function progressOf(s: NonNullable<typeof state>, op: Op): AddProgress {
	if (op.cancelled) return { phase: 'cancelled' };
	const t = Date.now() - op.startedAt;
	if (op.waiting)
		return { phase: 'waiting', waitingFor: ['Kai Lindqvist', 'Juno Park', 'Noor Haddad'] };
	if (op.failed || (query().get('add') === 'fails' && !s.failedOnce)) {
		s.failedOnce = true;
		op.failed = true;
		return {
			phase: 'failed',
			code: 'failed',
			reason: "Office 2 couldn't be created: the instance has no room for another office process."
		};
	}
	if (t < 1400) return { phase: 'creating' };
	if (t < 2800) return { phase: 'synchronizing' };
	if (!op.office) {
		const id = `office-${s.orgId}-${op.id}`;
		op.office = {
			id,
			organization_id: s.orgId,
			name: op.name,
			role: 'satellite',
			host: 'This Mac',
			endpoint: 'http://127.0.0.1:7421',
			status: 'starting'
		};
		s.satellites = [...s.satellites, op.office];
		s.details[id] = {
			officeId: id,
			where: 'HQ instance',
			lastSeenAt: new Date().toISOString(),
			sync: { kind: 'up_to_date' },
			follows: [],
			tasks: 0
		};
	}
	return { phase: 'ready', office: op.office };
}

export const officesApi: OfficesApi = {
	async overview(organizationId) {
		if (organizationId === POC_ORG_ID) {
			await wait();
			return structuredClone(pocOfficesOverview());
		}
		const s = await gate(organizationId);
		const pendingAdds: PendingAdd[] = s.ops
			.filter((o) => o.waiting && !o.cancelled)
			.map((o) => ({
				id: o.id,
				name: o.name,
				instance: 'HQ instance',
				waitingFor: ['Kai Lindqvist', 'Juno Park', 'Noor Haddad']
			}));
		for (const o of s.ops)
			if (o.office && o.office.status === 'starting' && Date.now() - o.startedAt > 5200)
				s.satellites = s.satellites.map((x) =>
					x.id === o.office!.id ? { ...x, status: 'active' } : x
				);
		return {
			satellites: structuredClone(s.satellites),
			details: structuredClone(s.details),
			joinRequests: structuredClone(s.joinRequests),
			invitations: structuredClone(s.invitations),
			pendingAdds,
			outbox: structuredClone(s.outbox),
			placements: { ...s.placements },
			names: { ...s.names },
			viewer: { isAdministrator: query().get('admin') !== 'other' }
		} satisfies OfficesOverview;
	},
	async approveJoin(organizationId, requestId, code) {
		const s = await gate(organizationId);
		const request = on('gone') ? undefined : s.joinRequests.find((j) => j.id === requestId);
		if (!request)
			throw {
				source: 'app',
				code: 'not_found',
				message: 'That request is gone. It may have been declined or approved already.'
			};
		const normalize = (c: string) =>
			c
				.toUpperCase()
				.replace(/[\s-]/g, '')
				.replaceAll('O', '0')
				.replaceAll('I', '1')
				.replaceAll('L', '1');
		if (normalize(code) !== normalize(full.checkCode)) {
			const wrong = (s.wrongCodes[requestId] = (s.wrongCodes[requestId] ?? 0) + 1);
			if (wrong >= 3) {
				s.joinRequests = s.joinRequests.filter((j) => j.id !== requestId);
				s.invitations = [
					...s.invitations,
					{
						id: `inv-${requestId}`,
						officeName: request.officeName,
						state: 'cancelled',
						createdAt: request.invitationCreatedAt,
						expiresAt: ahead(0),
						note: `Join declined after 3 wrong codes. The invitation is cancelled; create a new invitation to try again.`
					}
				];
				throw {
					source: 'app',
					code: 'join_cancelled',
					message:
						'Join declined after 3 wrong codes. The invitation is cancelled; create a new invitation to try again.'
				};
			}
			throw {
				source: 'app',
				code: 'code_mismatch',
				triesLeft: 3 - wrong,
				message: `That code doesn't match ${request.officeName}'s. Check its terminal or your trusted session. ${3 - wrong} ${3 - wrong === 1 ? 'try' : 'tries'} left.`
			};
		}
		s.joinRequests = s.joinRequests.filter((j) => j.id !== requestId);
		s.joinRequests.forEach(
			(j) =>
				j.officeName === request.officeName &&
				(s.joinRequests = s.joinRequests.filter((x) => x.id !== j.id))
		);
		const id = `office-${organizationId}-${requestId}`;
		s.satellites = [
			...s.satellites,
			{
				id,
				organization_id: organizationId,
				name: request.officeName,
				role: 'satellite',
				host: request.officeName,
				status: 'starting'
			}
		];
		s.details[id] = {
			officeId: id,
			where: `from ${request.fromAddress}`,
			lastSeenAt: new Date().toISOString(),
			sync: { kind: 'behind', changes: 212 },
			follows: [],
			tasks: 0
		};
		setTimeout(() => {
			s.satellites = s.satellites.map((o) => (o.id === id ? { ...o, status: 'active' } : o));
			s.details[id] = { ...s.details[id], sync: { kind: 'up_to_date' } };
		}, 6000);
	},
	async declineJoin(organizationId, requestId) {
		const s = await gate(organizationId);
		const request = s.joinRequests.find((j) => j.id === requestId);
		s.joinRequests = s.joinRequests.filter((j) => j.id !== requestId);
		if (request)
			s.invitations = [
				...s.invitations,
				{
					id: `inv-${requestId}`,
					officeName: request.officeName,
					state: 'rejected',
					createdAt: request.invitationCreatedAt,
					expiresAt: ahead(0),
					note: `The join for ${request.officeName} was declined at HQ. Nothing was added to this organization.`
				}
			];
	},
	async invite(organizationId, name) {
		const s = await gate(organizationId);
		if (nameTaken(s, name))
			throw {
				source: 'app',
				code: 'already_exists',
				message: `There's already an office called ${name.trim()}. Choose another name.`
			};
		const invitation: Invitation = {
			id: `inv-${Date.now()}`,
			officeName: name.trim(),
			state: 'invited',
			createdAt: new Date().toISOString(),
			expiresAt: ahead(inviteTtl)
		};
		s.invitations = [...s.invitations, invitation];
		return { invitation, secret: secret() };
	},
	async newInvitation(organizationId, invitationId) {
		const s = await gate(organizationId);
		const old = on('gone') ? undefined : s.invitations.find((i) => i.id === invitationId);
		if (!old)
			throw { source: 'app', code: 'not_found', message: "That invitation isn't here any more." };
		s.invitations = s.invitations.filter((i) => i.id !== invitationId);
		const invitation: Invitation = {
			id: `inv-${Date.now()}`,
			officeName: old.officeName,
			state: 'invited',
			createdAt: new Date().toISOString(),
			expiresAt: ahead(inviteTtl)
		};
		s.invitations = [...s.invitations, invitation];
		return { invitation, secret: secret() };
	},
	async cancelInvitation(organizationId, invitationId) {
		const s = await gate(organizationId);
		s.invitations = s.invitations.filter((i) => i.id !== invitationId);
	},
	async removeInvitation(organizationId, invitationId) {
		const s = await gate(organizationId);
		s.invitations = s.invitations.filter((i) => i.id !== invitationId);
	},
	async addOnInstance(organizationId, name) {
		const s = await gate(organizationId);
		if (query().get('admin') === 'other')
			throw {
				source: 'app',
				code: 'not_administrator',
				message:
					"Only this HQ's administrator can add an office on this instance. You can still invite another machine."
			};
		if (nameTaken(s, name))
			throw {
				source: 'app',
				code: 'already_exists',
				message: `There's already an office called ${name.trim()}. Choose another name.`
			};
		const op: Op = {
			id: `add-${Date.now()}`,
			name: name.trim(),
			startedAt: Date.now(),
			waiting: query().get('add') === 'waiting',
			cancelled: false
		};
		s.ops.push(op);
		return { operationId: op.id };
	},
	async addProgress(organizationId, operationId) {
		const s = await ensure(organizationId);
		if (query().get('add') === 'check-fails' && !checkFailed) {
			checkFailed = true;
			throw {
				source: 'app',
				code: 'unreachable',
				message: "HQ didn't answer. Check that it's running, then try again."
			};
		}
		const op = s.ops.find((o) => o.id === operationId);
		if (!op) return { phase: 'cancelled' };
		return progressOf(s, op);
	},
	async cancelAdd(organizationId, operationId) {
		const s = await gate(organizationId);
		const op = s.ops.find((o) => o.id === operationId);
		if (!op) return true;
		if (!op.waiting && Date.now() - op.startedAt > 1400) return false;
		op.cancelled = true;
		return true;
	},
	async removeOffice(organizationId, officeId) {
		const s = await gate(organizationId);
		s.satellites = s.satellites.filter((o) => o.id !== officeId);
		for (const [member, where] of Object.entries(s.placements))
			if (where === officeId) s.placements[member] = null;
		s.outbox = s.outbox.filter((o) => o.officeId !== officeId);
		delete s.details[officeId];
	},
	async renameOffice(organizationId, officeId, name) {
		const s = await gate(organizationId);
		if (
			nameTaken(s, name) &&
			(s.names[officeId] ?? s.satellites.find((o) => o.id === officeId)?.name)?.toLowerCase() !==
				name.trim().toLowerCase()
		)
			throw {
				source: 'app',
				code: 'already_exists',
				message: `There's already an office called ${name.trim()}. Choose another name.`
			};
		s.names[officeId] = name.trim();
	},
	async placeAgent(organizationId, memberId, officeId) {
		const s = await gate(organizationId);
		s.placements[memberId] = officeId;
	},
	async retryOutbox(organizationId, itemId) {
		const s = await gate(organizationId);
		const item = s.outbox.find((o) => o.id === itemId);
		if (!item) return;
		const office = s.satellites.find((o) => o.id === item.officeId);
		if (office?.status === 'offline')
			throw {
				source: 'app',
				code: 'blocked',
				message: `${item.officeName} is still offline. Try again when it's back.`
			};
		s.outbox = s.outbox.filter((o) => o.id !== itemId);
	},
	async discardOutbox(organizationId, itemId) {
		const s = await gate(organizationId);
		s.outbox = s.outbox.filter((o) => o.id !== itemId);
	}
};
