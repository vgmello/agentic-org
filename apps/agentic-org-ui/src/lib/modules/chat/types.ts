export type {
	AskOption,
	AskQuestion,
	ChatAccepted,
	ChatCall,
	ChatCallStatus,
	ChatMessage,
	ChatSnapshot,
	ChatState,
	Conversation,
	MuteMode,
	Presence,
	QuestionAnswer,
	QuestionType,
	Reaction,
	Withdrawal
} from '$lib/modules/messaging/types';

export type PendingDeliveryStatus = 'sending' | 'failed' | 'unknown' | 'checking';

export interface PendingMessage {
	key: number;
	/** Kept across a Retry, so a message the office already has is not posted twice. */
	change_id: string;
	conversation_id: string;
	thread_root_id?: string;
	also_in_channel?: boolean;
	body: string;
	attachments: string[];
	status: PendingDeliveryStatus;
	error?: string;
}

export type ChatPanelKind = 'thread' | 'members' | 'pins';

export interface ChatLocation {
	conversation?: string;
	message?: string;
	thread?: string;
	view?: string;
	query?: string;
}
