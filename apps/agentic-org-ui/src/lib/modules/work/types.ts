import type {
	BugAssign,
	BugClose,
	BugMarkFixed,
	BugReopen,
	BugReport,
	BugTriage,
	BugUpdate,
	BugVerify,
	ChecklistStep,
	NewStep,
	RequestAccept,
	RequestApprove,
	RequestAskChanges,
	RequestCreate,
	RequestDecline,
	RequestMerge,
	RequestOutcome,
	RequestRoute,
	RequestScope,
	RequestUpdate,
	RoutedBy,
	TaskAssign,
	TaskCreate,
	TaskSetStatus,
	TaskUpdate,
	WorkAccepted,
	WorkCommentCall,
	WorkEvent,
	WorkItemSummary,
	WorkLinkCall,
	WorkUnlink
} from '$lib/proto';

export interface StoredTask {
	summary: WorkItemSummary;
	description?: string;
	definition_of_done?: string;
	done_anyway?: { reason: string; steps: string[] };
	/** Bugs: what was reported and how it ended. */
	what_happened?: string;
	expected?: string;
	steps?: string;
	environment?: string;
	closed_reason?: string;
	/** When it first went In progress, and when it last went Done (a round closes at Done). */
	started_at?: string;
	done_at?: string;
	checklist: ChecklistStep[];
	watchers: string[];
	/** Links this item makes (`fixes`: a task to the bug it fixes). */
	links?: {
		id: string;
		kind: 'fixes' | 'from_request';
		to: string;
		created_by: string;
		created_at: string;
	}[];
	/** Requests: the original words, why this scoper, how it was routed, and how it ended. */
	text?: string;
	route_reason?: string;
	routed_by?: RoutedBy;
	outcome?: RequestOutcome;
	merged_into?: string;
	decline_reason?: string;
	/** Requests: what happened to it, oldest first (`work.activity`). */
	events?: WorkEvent[];
}

export interface WorkState {
	incarnation: number;
	seq: number;
	/** Next task number per project. */
	next: Record<string, number>;
	items: Record<string, StoredTask>;
	/** Answers to change ids already applied, so a repeat changes nothing. */
	answers: Record<string, { call: string; accepted: WorkAccepted }>;
}

/** What the engine needs to know about the world around the items. */
export interface WorkContext {
	now: string;
	/** The bound member making the call. */
	actor: string;
	projects: {
		id: string;
		archived?: boolean;
		teamIds: string[];
		areas?: { id: string; general?: boolean; deleted?: boolean }[];
	}[];
	/** Members who can be assigned (not offboarded). */
	members: string[];
}

export type WorkCall =
	| ({ method: 'task.create' } & TaskCreate)
	| ({ method: 'task.update' } & TaskUpdate)
	| ({ method: 'task.assign' } & TaskAssign)
	| ({ method: 'task.set_status' } & TaskSetStatus)
	| ({ method: 'bug.report' } & BugReport)
	| ({ method: 'bug.update' } & BugUpdate)
	| ({ method: 'bug.triage' } & BugTriage)
	| ({ method: 'bug.assign' } & BugAssign)
	| ({ method: 'bug.mark_fixed' } & BugMarkFixed)
	| ({ method: 'bug.verify' } & BugVerify)
	| ({ method: 'bug.close' } & BugClose)
	| ({ method: 'bug.reopen' } & BugReopen)
	| ({ method: 'request.create' } & RequestCreate)
	| ({ method: 'request.update' } & RequestUpdate)
	| ({ method: 'request.route' } & RequestRoute)
	| ({ method: 'request.accept' } & RequestAccept)
	| ({ method: 'request.scope' } & RequestScope)
	| ({ method: 'request.ask_changes' } & RequestAskChanges)
	| ({ method: 'request.approve' } & RequestApprove)
	| ({ method: 'request.decline' } & RequestDecline)
	| ({ method: 'request.merge' } & RequestMerge)
	| ({ method: 'work.link' } & WorkLinkCall)
	| ({ method: 'work.unlink' } & WorkUnlink)
	| ({ method: 'work.comment' } & WorkCommentCall)
	| {
			method: 'checklist.add';
			change_id: string;
			item_id: string;
			steps: NewStep[];
			after_step_id?: string;
	  }
	| { method: 'checklist.check'; change_id: string; step_id: string; done: boolean }
	| {
			method: 'checklist.edit';
			change_id: string;
			step_id: string;
			expected_version: number;
			text?: string;
			required?: boolean;
	  }
	| { method: 'checklist.remove'; change_id: string; step_id: string; expected_version: number }
	| { method: 'work.delete'; change_id: string; item_id: string; expected_version: number };
