// Chat as ADR-0003 section 6 sketches it (amended 2026-11-08: chat, proposed) and docs/specs/chat-ux.md specifies it. A mirror
// to build against until the Rust types land in crates/org-proto; replace it with the real shapes then.
// Unknown enum values from a newer office decode as 'unknown' and are never written.

export type ConversationKind = 'dm' | 'group_dm' | 'channel' | 'unknown';

/**
 * ADR-0003's vocabulary. The ask is `request` (urgent, blocking, shown as "Ask"); `msg` is a person's ordinary message.
 * There is no chat kind of its own for feature requests (REQ-n): those are a different object.
 */
export type MessageKind = 'msg' | 'request' | 'handoff' | 'announcement' | 'reply' | 'unknown';

/** What an agent may post (the office refuses anything else, section 6.5). */
export const AGENT_KINDS = ['request', 'handoff', 'announcement', 'reply'] as const;

export const ANNOUNCEMENT_REASONS = ['delivered', 'ready_for_review', 'blocked', 'report', 'decision_needed'] as const;
export type AnnouncementReason = (typeof ANNOUNCEMENT_REASONS)[number];
export const REASON_LABEL: Record<AnnouncementReason, string> = {
	delivered: 'Delivered',
	ready_for_review: 'Ready for review',
	blocked: 'Blocked',
	report: 'Report',
	decision_needed: 'Decision needed'
};

export type AutoChannel = { type: 'general' } | { type: 'project'; project_id: string } | { type: 'team'; team_id: string };

export interface Conversation {
	id: string;
	kind: ConversationKind;
	/** Channels only: lowercase, no spaces. */
	name?: string;
	members: string[];
	archived?: boolean;
	/** Set on the channels the office keeps in sync with the organization. */
	auto?: AutoChannel;
	created_at: string;
}

export interface AskOption {
	id: string;
	label: string;
}

export type Reaction = 'seen' | 'looking';
export const REACTIONS: { id: Reaction; label: string }[] = [
	{ id: 'seen', label: 'Seen' },
	{ id: 'looking', label: 'Looking' }
];

export type Bridge = 'slack' | 'teams' | 'discord';

/**
 * What a bridge connection is bound as (ADR-0003 6.6): its own binding, `bridge:<id>`, made with a bridge token the app issues.
 * It reaches only the channels chosen for it, never a DM or group DM, and posts as a mapped person who is a member.
 */
export interface BridgeBinding {
	id: string;
	platform: Bridge;
	/** Conversation ids of the channels chosen for this bridge. */
	channels: string[];
	/** Who it may post as: platform users mapped to members. Persons only, never an agent or the owner (checked at `bridge.create`). */
	people: { platform_user: string; member_id: string }[];
}

export interface Answer {
	/** A member id, or `default` when the ask's default was applied at its deadline. */
	by: string;
	at: string;
	option_id?: string;
	text?: string;
}

export interface ChatMessage {
	id: string;
	conversation_id: string;
	/** The message this one replies under (a thread has one level). */
	thread_root_id?: string;
	/** A thread reply that was also sent to the channel ("Also send to #channel"). */
	also_in_channel?: boolean;
	from: string;
	kind: MessageKind;
	body: string;
	created_at: string;
	/** `announcement` only. */
	reason?: AnnouncementReason;
	/** `request` and `handoff`: who is asked (a DM's other member when absent). */
	addressed_to?: string;
	/** `request` only. */
	options?: AskOption[];
	default_option?: string;
	/** RFC 3339: when the default is applied if nobody answered. */
	default_at?: string;
	/** What the ask blocks, e.g. "W7". */
	blocks?: string;
	/** An ask with Approve and Reject, answered only in the app. */
	approval?: boolean;
	answer?: Answer;
	/** `handoff`: who accepted it, and when. */
	accepted_by?: string;
	accepted_at?: string;
	reactions?: Partial<Record<Reaction, string[]>>;
	/** An extra announcement that was folded into a thread under the last one. */
	folded?: boolean;
	/** The platform a message came from; it was posted as the bridged person (decision 11). */
	bridge?: Bridge;
	attachments?: string[];
	pinned?: boolean;
	/** The task or bug a hand-off is linked to. */
	linked_ref?: string;
}

export type Presence = 'working' | 'waiting' | 'asleep' | 'offline' | 'paused' | 'online';

export type MuteMode = 'none' | 'muted' | 'digest';
/** The owner's setting for one agent in one conversation (per-viewer office data). */
export interface Mute {
	conversation_id: string;
	member_id: string;
	mode: MuteMode;
}

/** Who is typing: an agent woken by a message, until `until`. Not stored by the office. */
export interface Typing {
	conversation_id: string;
	member_id: string;
	until: string;
}

export interface ChatState {
	seq: number;
	conversations: Conversation[];
	messages: ChatMessage[];
	mutes: Mute[];
	/** The owner's read marker per conversation (RFC 3339). */
	read_at: Record<string, string>;
	presence: Record<string, Presence>;
	typing: Typing[];
	/** Change ids already applied, oldest first: a repeated call changes nothing. */
	applied: Record<string, ChatAccepted>;
}

// ---- calls (the sketch in ADR-0003 6.7) -------------------------------------------------------------

export type ChatCall =
	| {
			method: 'msg.send';
			change_id: string;
			conversation_id: string;
			thread_root_id?: string;
			/** Send a thread reply to the channel too. */
			also_in_channel?: boolean;
			/** An agent's reply to a message: it lands in that message's thread. */
			reply_to?: string;
			kind: MessageKind;
			body: string;
			reason?: AnnouncementReason;
			addressed_to?: string;
			options?: AskOption[];
			default_option?: string;
			default_at?: string;
			blocks?: string;
			approval?: boolean;
			attachments?: string[];
			linked_ref?: string;
	  }
	| { method: 'chat.answer'; change_id: string; ask_id: string; option_id?: string; text?: string }
	| { method: 'chat.accept_handoff'; change_id: string; message_id: string }
	| { method: 'chat.react'; change_id: string; message_id: string; reaction: Reaction }
	| { method: 'chat.mute'; change_id: string; conversation_id: string; member_id: string; mode: MuteMode }
	| { method: 'chat.mark_read'; change_id: string; conversation_id: string }
	| { method: 'chat.pin'; change_id: string; message_id: string }
	| { method: 'dm.open'; change_id: string; members: string[] }
	| { method: 'channel.create'; change_id: string; name: string; members: string[] }
	| { method: 'channel.rename'; change_id: string; id: string; name: string }
	| { method: 'channel.archive'; change_id: string; id: string }
	| { method: 'channel.add_member'; change_id: string; id: string; member_id: string }
	| { method: 'channel.remove_member'; change_id: string; id: string; member_id: string };

export interface ChatAccepted {
	/** The new message, or the new conversation for `dm.open` and `channel.create`. */
	id?: string;
	conversation_id?: string;
	/** An extra announcement: the thread root it was folded into. */
	folded_into?: string;
	/** Agents the message woke from sleep. */
	woke?: string[];
}

/** The errors chat adds to `ErrorCode` (ADR-0003 6.7), next to the existing ones it reuses. */
export type ChatErrorCode =
	| 'kind_not_allowed'
	| 'reason_required'
	| 'conversation_archived'
	| 'too_many_members'
	| 'not_in_conversation'
	| 'not_authorized'
	| 'not_found'
	| 'invalid_request'
	| 'already_exists'
	| 'unavailable';

export interface ChatRefusal {
	source: 'api';
	code: ChatErrorCode;
	message: string;
}

export const CHAT_LIMITS = {
	/** Members in a group DM (CHAT-3). */
	groupDmMax: 8,
	askOptionsMax: 6,
	bodyMax: 256 * 1024,
	/** At most one announcement per agent per channel in this window (CHAT-10). */
	announcementWindowMs: 15 * 60 * 1000,
	/** Lines before a message collapses behind "Show more" (CHAT-14). */
	collapseLines: 15
} as const;

/** What the sidebar and the views need, as of one moment (`chat.snapshot`, a mock until the office serves chat). */
export interface ChatSnapshot {
	state: ChatState;
	/** True while the data is the TEMPORARY invented sample. */
	sample: boolean;
	/** The office isn't running: read-only from the last copy. */
	office_running: boolean;
}
