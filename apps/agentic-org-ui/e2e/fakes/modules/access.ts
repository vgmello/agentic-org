// Mock for src/lib/modules/access/api.ts under `bun run dev:mock`. It keeps no state of its own: people, handles and
// home teams come from the org mock and the members mock, who has an account or an invitation and who owns the
// organization from the members mock (signInOf, ownerIdOf), and the administrator and usernames from the settings mock.
// So an invitation or a removal made here, on a member profile or a hand-over on the org chart or in General shows the
// same on every page in one session. The sign-in actions are the members mock's (members.ts).
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (reads fail) · empty (no people) · unavailable · viewer (no manage rights)
//   invited (Leo Brandt has an open invitation) · deactivated (Hana Weiss is deactivated)
//   refuse (the sign-in actions are refused) · gone (the next action on Leo finds him gone: he leaves the list)
import { officeApi } from '$lib/office/api';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { AccessApi } from '../../../src/lib/modules/access/api';
import type { AccessOverview, AccessPerson } from '../../../src/lib/modules/access/types';
import { mockLatency, mockMode } from '../mock-mode';
import {
	handleOf,
	homeTeamId,
	isInactive,
	ownerIdOf,
	present,
	seedInvitation,
	signInOf,
	snapshotFor,
	viewerId
} from './members';
import { administratorAccount, usernameOf } from './settings';

const fail = (code: string, message: string) =>
	Object.assign(new Error(message), { source: 'api', code, message });

const modes = () => mockMode().split(',');

async function overview(organizationId: string): Promise<AccessOverview> {
	await mockLatency();
	if (modes().includes('fails'))
		throw fail('internal', "HQ didn't answer. Check that it's running, then try again.");
	if (modes().includes('unavailable')) await unavailableCall();
	if (modes().includes('invited')) seedInvitation('leo', 18 * 3_600_000);

	const s = await snapshotFor(organizationId);
	const connection = (await officeApi.connections()).find(
		(c) => c.organization.id === organizationId
	);
	const teamName = (id: string | undefined) => s.teams.find((t) => t.id === id)?.name;
	const owner = ownerIdOf(s);
	const admin = administratorAccount();
	const viewer = viewerId();
	const blocked = modes().includes('viewer');

	const people: AccessPerson[] = modes().includes('empty')
		? []
		: s.members
				.filter((m) => m.kind === 'person' && present(m))
				.map((m) => {
					const access = signInOf(m.id);
					const active = !isInactive(m);
					const homeTeam = teamName(homeTeamId(s, m.id));
					return {
						memberId: m.id,
						name: m.name,
						handle: handleOf(m),
						...(homeTeam ? { homeTeam } : {}),
						owner: m.id === owner,
						administrator: m.id === admin.memberId,
						active,
						state: access.account ? 'signed_in' : access.invitedUntil ? 'invited' : 'no_account',
						...(access.account ? { username: usernameOf(m.id) ?? handleOf(m) } : {}),
						...(access.invitedUntil ? { inviteExpiresAt: access.invitedUntil } : {})
					};
				});

	const adminPerson = people.find((p) => p.memberId === admin.memberId);
	const isOwner = !blocked && viewer === owner;
	const isAdministrator = !blocked && viewer === admin.memberId;
	return {
		organization: { id: organizationId, name: connection?.organization.name ?? 'Organization' },
		administrator: {
			username: admin.username,
			...(adminPerson ? { memberId: adminPerson.memberId, name: adminPerson.name } : {}),
			isYou: isAdministrator
		},
		people,
		viewer: { isOwner, isAdministrator, canManage: isOwner || isAdministrator }
	};
}

export const accessApi: AccessApi = { overview };
