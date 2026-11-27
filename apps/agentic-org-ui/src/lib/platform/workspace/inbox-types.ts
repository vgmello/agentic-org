/** Shared inbox item contract used across shell data context and domain modules. */
import type { PendingExternalAuthView, WorkspaceFamiliarity } from './external-auth';

export type InboxKind = 'approval' | 'question' | 'decision' | 'problem';

export const KIND_WORD: Record<InboxKind, string> = {
	approval: 'Approval',
	question: 'Question',
	decision: 'Decision',
	problem: 'Problem'
};

/** What the owner can do to an item here. Absent means the source offers no such operation: the card offers Open instead. */
export type InboxAction = 'approve' | 'deny' | 'reply' | 'choose' | 'dismiss';

export interface InboxPerson {
	id: string;
	name: string;
	kind: 'agent' | 'person';
}

export type InboxWorkspaceFamiliarity = WorkspaceFamiliarity;

/** Pending BYOA external-agent sign-in metadata attached to Inbox approval items (`TASK-249`). */
export type InboxExternalAuthMeta = PendingExternalAuthView;

export interface InboxItem {
	id: string;
	kind: InboxKind;
	/** The source in words: "Run approval", "Task question", "Run crashed". */
	source: string;
	/** What it is. */
	title: string;
	/** Who it comes from. */
	who: InboxPerson;
	team?: { id: string; name: string };
	project?: { id: string; name: string };
	/** Why it needs the owner. */
	why: string;
	recommendation?: string;
	created_at: string;
	/** True when the backing gateway omitted a recorded timestamp and the adapter filled in the read time. */
	created_at_inferred?: boolean;
	/** When it lapses, when the source says so. */
	expires_at?: string;
	actions: InboxAction[];
	/** Optional domain-supplied primary approval button label; defaults to "Approve" when omitted. */
	approve_label?: string;
	/** The options of a decision. */
	choices?: { id: string; label: string }[];
	/** Where the item's subject lives, for Open. */
	open?: { path: string; label: string };
	/** Whether the item's backing source supports snooze/un-snooze (`TASK-81`); defaults to true when omitted. */
	snoozeable?: boolean;
	state: 'open' | 'snoozed' | 'handled';
	/** RFC 3339 timestamp when a snoozed item returns to the open inbox (`TASK-81`). */
	snoozed_until?: string;
	/** Optional snooze note recorded when deferring the item (`TASK-81`). */
	snooze_note?: string;
	handled?: { outcome: string; at: string; by?: string };
	/** Present when this inbox item represents a pending BYOA external-agent sign-in request (`TASK-249`). */
	external_auth?: InboxExternalAuthMeta;
}
