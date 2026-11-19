// What Settings › Members and access reads (server-installation-and-access-ux §4a, offices-and-teams-ux §5b, §7.1,
// §7.4). An account is held by the host and tied, in this organization, to one person; agents never sign in. The
// sign-in actions themselves (Invite to sign in, New sign-in link, Remove access) belong to the members module.

/** A person's sign-in state in this organization, in words on the page. */
export type AccessState = 'signed_in' | 'invited' | 'no_account';

export interface AccessPerson {
	memberId: string;
	name: string;
	/** Stored without `@`. */
	handle: string;
	/** The name of the person's one home team. Absent only for an invalid record, which the page says. */
	homeTeam?: string;
	/** The organization's owner: ownership moves on the org chart (§7.1). */
	owner: boolean;
	/** The instance's administrator account is tied to this person. */
	administrator: boolean;
	/**
	 * False once the person is deactivated (the member profile's `status: 'inactive'`). Deactivating a person also
	 * removes their access (§7.4), so the state is `no_account` and no sign-in action is offered.
	 */
	active: boolean;
	state: AccessState;
	/** Set when `state` is `signed_in`: the account's username. */
	username?: string;
	/** Set when `state` is `invited`: RFC 3339. The link works until then. */
	inviteExpiresAt?: string;
}

/** The instance's administrator, who may be a person here or none yet. */
export interface AccessAdministrator {
	username: string;
	/** The person the account is tied to in this organization, when it has one. */
	memberId?: string;
	name?: string;
	/** The viewer is the administrator. */
	isYou: boolean;
}

export interface AccessOverview {
	organization: { id: string; name: string };
	administrator: AccessAdministrator;
	/** Every person in the organization, deactivated people included. Agents are never listed: they have no account. */
	people: AccessPerson[];
	viewer: {
		/** The viewer holds ownership: the owner's fact links to the org chart, where ownership moves. */
		isOwner: boolean;
		/** The viewer is the instance's administrator: the administrator's fact links to General. */
		isAdministrator: boolean;
		/** The owner or the administrator: sign-in actions are offered. Everyone else sees the lists only. */
		canManage: boolean;
	};
}
