import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { workItemsApi } from '$lib/modules/workitems/api';
import { createHttpMembersApi } from './http-api';
import type {
	ActivityEntry,
	AssignableTask,
	ConversationRow,
	MemberProfile,
	MemberRow,
	RoleView,
	RunNowResult,
	SessionFile,
	SignInLink,
	Turn,
	WorkRow
} from './types';

/**
 * What the Members directory, the member profile and the session log read and change. Errors are `AppError`s:
 * `busy` and `cannot_start` from runNow, `wrong_password` from handOverOwnership, `not_authorized` anywhere.
 */
export interface MembersApi {
	/** Everyone in the organization, people and agents, former members included. */
	directory(organizationId: string): Promise<MemberRow[]>;
	profile(organizationId: string, memberId: string): Promise<MemberProfile>;
	work(organizationId: string, memberId: string): Promise<WorkRow[]>;
	conversations(organizationId: string, memberId: string): Promise<ConversationRow[]>;
	activity(organizationId: string, memberId: string): Promise<ActivityEntry[]>;
	/** Agents only: newest first. */
	runs(organizationId: string, memberId: string): Promise<Turn[]>;
	/**
	 * Proof of concept only (REQ-05): the run's conversation file, parsed in the browser by features/session-log.
	 * The product never serves a raw transcript: the office parses and redacts, and this call is replaced by one
	 * returning the parsed `SessionLog`. `not_found` when no log was kept.
	 */
	sessionFile(organizationId: string, memberId: string, runId: string): Promise<SessionFile>;
	role(organizationId: string, memberId: string): Promise<RoleView>;
	/**
	 * Run now, with an optional note. Rejects `busy` while a turn is open (RUN-4) and `cannot_start` with the reason.
	 * Answers the recorded turn (`accepted` until the office confirms the start) and the profile after it.
	 */
	runNow(organizationId: string, memberId: string, note: string): Promise<RunNowResult>;
	setAllowHiring(organizationId: string, memberId: string, on: boolean): Promise<void>;
	/**
	 * Move home team (our-org-in-the-app §8, TEAM-6): one operation that records `toTeamId` as the employee's one home
	 * team. Refuses `conflict` when it is already their home team or they are already in that team.
	 */
	moveHomeTeam(organizationId: string, memberId: string, toTeamId: string): Promise<void>;
	assignableTasks(organizationId: string): Promise<AssignableTask[]>;
	assignTask(organizationId: string, memberId: string, ref: string): Promise<void>;
	/*
	 * Sign-in access (offices-and-teams-ux §7.4): the one contract for it. The member profile and Settings › Members and
	 * access both call these through SignInAccessDialogs. Each refuses `not_authorized` for anyone but the owner or the
	 * administrator, and `not_found` for a person no longer in the organization.
	 */
	/** A one-time registration link, shown once. Inviting again cancels the previous link. Refused for a deactivated person. */
	inviteToSignIn(organizationId: string, memberId: string): Promise<SignInLink>;
	/** A one-time set-new-password link, shown once. Refused for anyone without an account. */
	newSignInLink(organizationId: string, memberId: string): Promise<SignInLink>;
	/** Signs the person out everywhere. Refused `conflict` for the owner ("Hand over ownership first."), and without an account. */
	removeAccess(organizationId: string, memberId: string): Promise<void>;
	/** Moves ownership in one operation, after the owner's password. */
	handOverOwnership(organizationId: string, toMemberId: string, password: string): Promise<void>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayMembersApi = createHttpMembersApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	workItemsApi,
	session: () => {
		const s = signedInSession();
		return s ? { accountAdmin: s.admin } : undefined;
	}
});

// Production members API backed by `/api/v1/organizations/{org}` snapshot, mutations and access routes.
// Under `bun run dev:mock`, e2e/fakes/modules/members.ts stands in for this file.
export const membersApi: MembersApi = Object.freeze<MembersApi>({
	directory: (orgId) =>
		signedInSession() ? gatewayMembersApi.directory(orgId) : unavailableCall(),
	profile: (orgId, memberId) =>
		signedInSession() ? gatewayMembersApi.profile(orgId, memberId) : unavailableCall(),
	work: (orgId, memberId) =>
		signedInSession() ? gatewayMembersApi.work(orgId, memberId) : unavailableCall(),
	conversations: (orgId, memberId) =>
		signedInSession() ? gatewayMembersApi.conversations(orgId, memberId) : unavailableCall(),
	activity: (orgId, memberId) =>
		signedInSession() ? gatewayMembersApi.activity(orgId, memberId) : unavailableCall(),
	runs: (orgId, memberId) =>
		signedInSession() ? gatewayMembersApi.runs(orgId, memberId) : unavailableCall(),
	sessionFile: unavailableCall,
	role: (orgId, memberId) =>
		signedInSession() ? gatewayMembersApi.role(orgId, memberId) : unavailableCall(),
	runNow: unavailableCall,
	setAllowHiring: unavailableCall,
	moveHomeTeam: (orgId, memberId, toTeamId) =>
		signedInSession()
			? gatewayMembersApi.moveHomeTeam(orgId, memberId, toTeamId)
			: unavailableCall(),
	assignableTasks: (orgId) =>
		signedInSession() ? gatewayMembersApi.assignableTasks(orgId) : unavailableCall(),
	assignTask: unavailableCall,
	inviteToSignIn: (orgId, memberId) =>
		signedInSession() ? gatewayMembersApi.inviteToSignIn(orgId, memberId) : unavailableCall(),
	newSignInLink: (orgId, memberId) =>
		signedInSession() ? gatewayMembersApi.newSignInLink(orgId, memberId) : unavailableCall(),
	removeAccess: (orgId, memberId) =>
		signedInSession() ? gatewayMembersApi.removeAccess(orgId, memberId) : unavailableCall(),
	handOverOwnership: (orgId, toMemberId, password) =>
		signedInSession()
			? gatewayMembersApi.handOverOwnership(orgId, toMemberId, password)
			: unavailableCall()
});
