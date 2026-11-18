import { unavailableCall } from '$lib/platform/unavailable-api';
import type { SessionLog } from './features/session-log/entries';
import type {
	ActivityEntry,
	AssignableTask,
	ConversationRow,
	MemberProfile,
	MemberRow,
	ProcessesView,
	RoleView,
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
	/** The office parses and redacts the conversation files; the browser never reads one (REQ-05). */
	sessionLog(organizationId: string, memberId: string, runId: string): Promise<SessionLog>;
	processes(organizationId: string, memberId: string): Promise<ProcessesView>;
	role(organizationId: string, memberId: string): Promise<RoleView>;
	/** Run now, with an optional note. Rejects `busy` while a turn is open (RUN-4) and `cannot_start` with the reason. */
	runNow(organizationId: string, memberId: string, note: string): Promise<Turn>;
	setAllowHiring(organizationId: string, memberId: string, on: boolean): Promise<void>;
	assignableTasks(organizationId: string): Promise<AssignableTask[]>;
	assignTask(organizationId: string, memberId: string, ref: string): Promise<void>;
	/** A one-time registration link, shown once. Inviting again cancels the previous link. */
	inviteToSignIn(organizationId: string, memberId: string): Promise<SignInLink>;
	/** A one-time set-new-password link, shown once. */
	newSignInLink(organizationId: string, memberId: string): Promise<SignInLink>;
	removeAccess(organizationId: string, memberId: string): Promise<void>;
	/** Moves ownership in one operation, after the owner's password. */
	handOverOwnership(organizationId: string, toMemberId: string, password: string): Promise<void>;
}

// The web API does not serve these yet: every call refuses. Under `bun run dev:mock`,
// e2e/fakes/modules/members.ts stands in for this file.
export const membersApi: MembersApi = {
	directory: unavailableCall,
	profile: unavailableCall,
	work: unavailableCall,
	conversations: unavailableCall,
	activity: unavailableCall,
	runs: unavailableCall,
	sessionLog: unavailableCall,
	processes: unavailableCall,
	role: unavailableCall,
	runNow: unavailableCall,
	setAllowHiring: unavailableCall,
	assignableTasks: unavailableCall,
	assignTask: unavailableCall,
	inviteToSignIn: unavailableCall,
	newSignInLink: unavailableCall,
	removeAccess: unavailableCall,
	handOverOwnership: unavailableCall
};
