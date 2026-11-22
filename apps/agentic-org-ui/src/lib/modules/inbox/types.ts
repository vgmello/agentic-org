/** One kind per source (our-org-in-the-app §6.1). */
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
	/** When it lapses, when the source says so. */
	expires_at?: string;
	actions: InboxAction[];
	/** Optional domain-supplied primary approval button label; defaults to "Approve" when omitted. */
	approve_label?: string;
	/** The options of a decision. */
	choices?: { id: string; label: string }[];
	/** Where the item's subject lives, for Open. */
	open?: { path: string; label: string };
	state: 'open' | 'handled';
	handled?: { outcome: string; at: string; by?: string };
}

/** The operation the owner asks for. The source decides; the card shows the source's reason when it refuses. */
export type InboxRequest =
	| { type: 'approve' }
	| { type: 'deny'; reason: string }
	| { type: 'reply'; text: string }
	| { type: 'choose'; choice: string }
	| { type: 'dismiss' };

export type InboxTab = 'all' | 'approvals' | 'questions' | 'decisions' | 'problems' | 'handled';

export const TABS: { id: InboxTab; label: string }[] = [
	{ id: 'all', label: 'All' },
	{ id: 'approvals', label: 'Approvals' },
	{ id: 'questions', label: 'Questions' },
	{ id: 'decisions', label: 'Decisions' },
	{ id: 'problems', label: 'Problems' },
	{ id: 'handled', label: 'Handled' }
];

/** The team / project filters of the Show menu. */
export interface InboxScope {
	team?: string;
	project?: string;
}
