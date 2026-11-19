// Mock for src/lib/modules/access/api.ts under `bun run dev:mock`. People, their home teams and handles come from the org
// mock and members data, so IDs and names match the other pages. Which people have an account, the administrator, the
// owner and the pending invitations are JSON in e2e/fakes/data/access. Changes are kept in memory until the page reloads.
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (reads fail) · empty (no people) · unavailable · viewer (no manage rights) · invited (Leo is invited)
//   refuse (issuing or removing is refused) · gone (the person has left: the next action on them refuses)
import { officeApi } from '$lib/office/api';
import { orgApi } from '$lib/org/api';
import type { AccessApi } from '../../../src/lib/modules/access/api';
import type {
	AccessOverview,
	AccessPerson,
	IssuedLink
} from '../../../src/lib/modules/access/types';
import { mockLatency, mockMode } from '../mock-mode';
import data from '../data/access/access.json' with { type: 'json' };
import membersData from '../data/members/members.json' with { type: 'json' };

const handles = membersData.members as Record<string, { handle?: string }>;
const fail = (code: string, message: string) =>
	Object.assign(new Error(message), { source: 'api', code, message });

const accounts = new Map<string, string>(Object.entries(data.accounts));
// Pending invitations start empty; `?mock=invited` seeds one for Leo Brandt, who has no account.
const invitations = new Map<string, string>();
const owner = data.owner;
const administrator = { ...data.administrator };

async function gate() {
	await mockLatency(250);
	if (mockMode().includes('fails'))
		throw fail('internal', "HQ didn't answer. Check that it's running, then try again.");
}

async function people(organizationId: string) {
	const { snapshot } = await orgApi.view(organizationId);
	const teamNames = new Map(snapshot.teams.map((t) => [t.id, t.name]));
	const homeOf = new Map<string, string>();
	for (const m of snapshot.memberships)
		if (!homeOf.has(m.member_id)) homeOf.set(m.member_id, m.team_id);
	return {
		organizationName: (await officeApi.connections()).find(
			(c) => c.organization.id === organizationId
		)?.organization.name,
		members: snapshot.members.filter((m) => m.kind === 'person'),
		teamName: (memberId: string) => teamNames.get(homeOf.get(memberId) ?? '') ?? 'No home team'
	};
}

async function overview(organizationId: string): Promise<AccessOverview> {
	await gate();
	if (mockMode().includes('unavailable'))
		throw fail('unavailable', "This server doesn't serve sign-in access yet.");
	const scope = await people(organizationId);
	if (mockMode().includes('invited') && !invitations.has('leo'))
		invitations.set('leo', new Date(Date.now() + 18 * 3_600_000).toISOString());
	const viewerIsOwner = owner === 'you';
	const viewer = mockMode().includes('viewer')
		? { isOwner: false, isAdministrator: false, canManage: false }
		: {
				isOwner: viewerIsOwner,
				isAdministrator: administrator.memberId === 'you',
				canManage: true
			};
	const list: AccessPerson[] = mockMode().includes('empty')
		? []
		: scope.members.map((m) => {
				const username = accounts.get(m.id);
				const invite = invitations.get(m.id);
				const state = username ? 'signed_in' : invite ? 'invited' : 'no_account';
				return {
					memberId: m.id,
					name: m.name,
					handle: handles[m.id]?.handle ?? m.id,
					homeTeam: scope.teamName(m.id),
					owner: m.id === owner,
					administrator: m.id === administrator.memberId,
					state,
					...(username ? { username } : {}),
					...(!username && invite ? { inviteExpiresAt: invite } : {})
				};
			});
	const adminMember = list.find((p) => p.memberId === administrator.memberId);
	return {
		organization: { id: organizationId, name: scope.organizationName ?? 'Organization' },
		administrator: {
			username: administrator.username,
			...(adminMember ? { memberId: adminMember.memberId, name: adminMember.name } : {}),
			isYou: administrator.memberId === 'you'
		},
		people: list,
		viewer
	};
}

function requireManager() {
	if (mockMode().includes('viewer') || mockMode().includes('refuse'))
		throw fail('not_authorized', 'Only the owner or the administrator changes sign-in access.');
}

export const accessApi: AccessApi = {
	overview,
	async issueLink(organizationId, memberId, kind): Promise<IssuedLink> {
		await mockLatency(400);
		requireManager();
		const scope = await people(organizationId);
		if (mockMode().includes('gone') || !scope.members.some((m) => m.id === memberId))
			throw fail('not_found', "That person isn't in this organization any more.");
		if (kind === 'reset' && !accounts.has(memberId))
			throw fail('conflict', 'Invite this person to sign in first.');
		if (kind === 'invite' && accounts.has(memberId))
			throw fail('conflict', 'This person already has an account.');
		const secret =
			Math.random().toString(36).slice(2, 14) + Math.random().toString(36).slice(2, 10);
		const expiresAt = new Date(Date.now() + 24 * 3_600_000).toISOString();
		if (kind === 'invite') invitations.set(memberId, expiresAt);
		return {
			url: `http://127.0.0.1:7420/${kind === 'invite' ? 'register' : 'set-password'}#${secret}`,
			expiresAt,
			kind
		};
	},
	async removeAccess(organizationId, memberId) {
		await mockLatency(400);
		requireManager();
		if (owner === memberId) throw fail('not_authorized', 'Hand over ownership first.');
		const scope = await people(organizationId);
		if (mockMode().includes('gone') || !scope.members.some((m) => m.id === memberId))
			throw fail('not_found', "That person isn't in this organization any more.");
		accounts.delete(memberId);
		invitations.delete(memberId);
	}
};
