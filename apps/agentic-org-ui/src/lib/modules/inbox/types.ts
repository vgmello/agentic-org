export {
	KIND_WORD,
	type InboxAction,
	type InboxItem,
	type InboxKind,
	type InboxPerson
} from '$lib/platform/workspace/inbox-types';

/** The operation the owner asks for. The source decides; the card shows the source's reason when it refuses. */
export type InboxRequest =
	| { type: 'approve' }
	| { type: 'deny'; reason: string }
	| { type: 'reply'; text: string }
	| { type: 'choose'; choice: string }
	| { type: 'dismiss' };

export type InboxTab =
	'all' | 'approvals' | 'questions' | 'decisions' | 'problems' | 'snoozed' | 'handled';

export const TABS: { id: InboxTab; label: string }[] = [
	{ id: 'all', label: 'All' },
	{ id: 'approvals', label: 'Approvals' },
	{ id: 'questions', label: 'Questions' },
	{ id: 'decisions', label: 'Decisions' },
	{ id: 'problems', label: 'Problems' },
	{ id: 'snoozed', label: 'Snoozed' },
	{ id: 'handled', label: 'Handled' }
];

/** The team / project filters of the Show menu. */
export interface InboxScope {
	team?: string;
	project?: string;
}
