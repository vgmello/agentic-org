// Mock for src/lib/modules/access/api.ts under `bun run dev:mock`. People, handles, home teams and who has an account come
// from the org mock and the members data (`canSignIn`, `homeTeamId`), so this page agrees with Members and the profiles.
// JSON in e2e/fakes/data/access adds only the owner, the administrator and usernames. Changes are kept in memory until
// the page reloads.
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (reads fail) · empty (no people) · unavailable · viewer (no manage rights)
//   invited (Leo Brandt is invited) · refuse (issuing or removing is refused) · gone (the next action on Leo finds
//   him gone: he leaves the list)
import { officeApi } from '$lib/office/api';
import { orgApi } from '$lib/org/api';
import type { AccessApi } from '../../../src/lib/modules/access/api';
import { NOT_HOME_TEAM } from '../../../src/lib/modules/access/present';
import type {
	AccessOverview,
	AccessPerson,
	IssuedLink
} from '../../../src/lib/modules/access/types';
import { mockLatency, mockMode } from '../mock-mode';
import data from '../data/access/access.json' with { type: 'json' };
import membersData from '../data/members/members.json' with { type: 'json' };

type MemberExtras = { handle?: string; canSignIn?: boolean; homeTeamId?: string };
const extras = membersData.members as Record<string, MemberExtras>;

const fail = (code: string, message: string) =>
	Object.assign(new Error(message), { source: 'api', code, message });

/** Invitations made in this page's session; `?mock=invited` seeds one for Leo Brandt. */
const invitations = new Map<string, string>();
const left = new Set<string>();
/** Accounts removed in this session (the members data is never changed). */
const removed = new Set<string>();
const hasAccount = (memberId: string) =>
	!removed.has(memberId) && extras[memberId]?.canSignIn === true;
const owner = data.owner;
const administrator = data.administrator;
const usernames = data.usernames as Record<string, string>;

async function gate() {
	await mockLatency(250);
	if (mockMode().includes('fails'))
		throw fail('internal', "HQ didn't answer. Check that it's running, then try again.");
}

async function people(organizationId: string) {
	const { snapshot } = await orgApi.view(organizationId);
	const teamNames = new Map(snapshot.teams.map((t) => [t.id, t.name]));
	const firstTeamOf = new Map<string, string>();
	for (const m of snapshot.memberships)
		if (!firstTeamOf.has(m.member_id)) firstTeamOf.set(m.member_id, m.team_id);
	const connection = (await officeApi.connections()).find(
		(c) => c.organization.id === organizationId
	);
	return {
		organizationName: connection?.organization.name,
		members: snapshot.members.filter((m) => m.kind === 'person' && !left.has(m.id)),
		homeTeam(memberId: string): string {
			const teamId = extras[memberId]?.homeTeamId ?? firstTeamOf.get(memberId);
			return (teamId && teamNames.get(teamId)) || NOT_HOME_TEAM;
		}
	};
}

async function overview(organizationId: string): Promise<AccessOverview> {
	await gate();
	if (mockMode().includes('unavailable'))
		throw fail('unavailable', "This server doesn't serve sign-in access yet.");
	const scope = await people(organizationId);
	if (mockMode().includes('invited') && !invitations.has('leo'))
		invitations.set('leo', new Date(Date.now() + 18 * 3_600_000).toISOString());
	const viewerCanManage = !mockMode().includes('viewer');
	const list: AccessPerson[] = mockMode().includes('empty')
		? []
		: scope.members.map((m) => {
				// Who can sign in is the members data's `canSignIn`: the same answer the Members directory gives.
				const signedIn = hasAccount(m.id);
				const invite = invitations.get(m.id);
				const state = signedIn ? 'signed_in' : invite ? 'invited' : 'no_account';
				return {
					memberId: m.id,
					name: m.name,
					handle: extras[m.id]?.handle ?? m.id,
					homeTeam: scope.homeTeam(m.id),
					owner: m.id === owner,
					administrator: m.id === administrator.memberId,
					state,
					...(signedIn ? { username: usernames[m.id] ?? extras[m.id]?.handle ?? m.id } : {}),
					...(!signedIn && invite ? { inviteExpiresAt: invite } : {})
				};
			});
	const adminIsHere = list.find((p) => p.memberId === administrator.memberId);
	return {
		organization: { id: organizationId, name: scope.organizationName ?? 'Organization' },
		administrator: {
			username: administrator.username,
			...(adminIsHere ? { memberId: adminIsHere.memberId, name: adminIsHere.name } : {}),
			isYou: viewerCanManage && administrator.memberId === 'you'
		},
		people: list,
		viewer: {
			isOwner: viewerCanManage && owner === 'you',
			isAdministrator: viewerCanManage && administrator.memberId === 'you',
			canManage: viewerCanManage
		}
	};
}

function requireManager() {
	if (!mockMode().includes('viewer') && !mockMode().includes('refuse')) return;
	throw fail('not_authorized', 'Only the owner or the administrator changes sign-in access.');
}

/** `?mock=gone`: the first action on Leo finds him gone from the organization. */
async function requirePresent(organizationId: string, memberId: string) {
	const scope = await people(organizationId);
	if (mockMode().includes('gone') && memberId === 'leo') left.add('leo');
	if (left.has(memberId) || !scope.members.some((m) => m.id === memberId))
		throw fail('not_found', "That person isn't in this organization any more.");
}

export const accessApi: AccessApi = {
	overview,
	async issueLink(organizationId, memberId, kind): Promise<IssuedLink> {
		await mockLatency(400);
		requireManager();
		await requirePresent(organizationId, memberId);
		const signedIn = hasAccount(memberId);
		if (kind === 'reset' && !signedIn)
			throw fail('conflict', 'Invite this person to sign in first.');
		if (kind === 'invite' && signedIn)
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
		await requirePresent(organizationId, memberId);
		removed.add(memberId);
		invitations.delete(memberId);
	}
};
