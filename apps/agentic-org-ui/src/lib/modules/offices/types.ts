// Offices beyond the one this app is connected to (docs/specs/offices-and-teams-ux.md §6): join requests, invitations,
// offices that joined, offices waiting for agents to stop, and what is queued for an office. Offices that are listed with the
// connection stay as they are; these are layered beside them.
import type { OfficeInfo } from '$lib/proto';

/** A computer used an invitation's link and waits for its check code at HQ (§3.1 Wants to join). */
export interface JoinRequest {
	id: string;
	officeName: string;
	fromAddress: string;
	/** RFC 3339: when the computer used the invitation. */
	requestedAt: string;
	/** RFC 3339: when the invitation was made ("created today at 14:02"). */
	invitationCreatedAt: string;
}

export type InvitationState = 'invited' | 'expired' | 'rejected' | 'cancelled';

export interface Invitation {
	id: string;
	officeName: string;
	state: InvitationState;
	createdAt: string;
	/** RFC 3339. Only an invited one counts down. */
	expiresAt: string;
	/** Why it ended, when it did: "Join declined after 3 wrong codes." */
	note?: string;
}

/** What HQ knows about an office that joined: where it is, freshness, sync and what it holds. */
export interface SatelliteDetails {
	officeId: string;
	/** "HQ instance" or "from 192.168.1.31". */
	where: string;
	lastSeenAt?: string;
	sync: { kind: 'up_to_date' } | { kind: 'behind'; changes: number };
	version?: string;
	/** The projects it follows (names). */
	follows: string[];
	/** Open tasks of the agents placed there: HQ takes them back when the office is removed. */
	tasks: number;
}

export type OutboxState = 'pending' | 'blocked';

/** A command or message an office has queued for HQ, and where it stands (§6.5). */
export interface OutboxItem {
	id: string;
	officeId: string;
	officeName: string;
	description: string;
	state: OutboxState;
	reason?: string;
	/** RFC 3339. */
	since: string;
}

/** An Add office › On this instance that is waiting for running agents to stop (§6.2). */
export interface PendingAdd {
	id: string;
	name: string;
	instance: string;
	waitingFor: string[];
}

export interface OfficesOverview {
	/** Offices that joined, listed beside the connection's own. */
	satellites: OfficeInfo[];
	details: Record<string, SatelliteDetails>;
	joinRequests: JoinRequest[];
	invitations: Invitation[];
	pendingAdds: PendingAdd[];
	outbox: OutboxItem[];
	/** Where each agent is placed (`null`: unplaced), over the organization's own record. */
	placements: Record<string, string | null>;
	/** New names, over the offices' own. */
	names: Record<string, string>;
	viewer: { isAdministrator: boolean };
}

export type AddPhase = 'waiting' | 'creating' | 'synchronizing' | 'ready' | 'failed' | 'cancelled';

export interface AddProgress {
	phase: AddPhase;
	/** The agents that must stop first, while `waiting`. */
	waitingFor?: string[];
	reason?: string;
	/** The refusal's code, so the dialog can offer the right next step. */
	code?: 'not_administrator' | 'failed';
	office?: OfficeInfo;
}

export interface Issued {
	invitation: Invitation;
	/** The invitation itself: shown once, never kept (§6.2). */
	secret: string;
}
