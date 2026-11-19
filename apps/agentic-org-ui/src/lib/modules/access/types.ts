// What Settings › Members and access reads and changes (server-installation-and-access-ux §4a, offices-and-teams-ux
// §5b, §7.1, §7.4). An account is held by the host and tied, in this organization, to one person; agents never sign in.

/** A person's sign-in state in this organization, in words on the page. */
export type AccessState = 'signed_in' | 'invited' | 'no_account';

export interface AccessPerson {
	memberId: string;
	name: string;
	/** Stored without `@`. */
	handle: string;
	/** The name of the person's home team. */
	homeTeam: string;
	/** The organization's owner: ownership moves on the org chart (§7.1). */
	owner: boolean;
	/** The instance's administrator account is tied to this person. */
	administrator: boolean;
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
	/** Every person in the organization. Agents are never listed: they have no account. */
	people: AccessPerson[];
	viewer: {
		/** The viewer holds ownership: the owner's row points to the org chart, where ownership moves. */
		isOwner: boolean;
		/** The viewer is the instance's administrator. */
		isAdministrator: boolean;
		/** The owner or the administrator: sign-in actions are offered. Everyone else sees the lists only. */
		canManage: boolean;
	};
}

/** A one-time link, shown once. */
export interface IssuedLink {
	url: string;
	/** RFC 3339. */
	expiresAt: string;
	/** A registration link (no account yet) or a set-new-password link (has an account). */
	kind: 'invite' | 'reset';
}
