import { dayLabel, parseChatLocation } from '$lib/modules/messaging';
import type { ChatMessage, Conversation } from '$lib/modules/messaging';
import type { OrgSnapshot } from '$lib/proto';
import { span } from '$lib/modules/work/time';
import type { ChatLocation, PendingMessage } from './types';

export function urlLocation(search: string): ChatLocation {
	const params = new URLSearchParams(search);
	return {
		...parseChatLocation(search),
		thread: params.get('thread') ?? undefined,
		view: params.get('v') ?? undefined,
		query: params.get('q') ?? ''
	};
}

export function followsWords(c: Conversation, snapshot: OrgSnapshot): string | undefined {
	if (c.auto?.type === 'general') return 'the organization';
	if (c.auto?.type === 'project') {
		const id = c.auto.project_id;
		const projectName = snapshot.projects?.find((p) => p.id === id)?.name;
		return `the ${projectName ?? 'project'} project`;
	}
	if (c.auto?.type === 'team') {
		const id = c.auto.team_id;
		const teamName = snapshot.teams.find((t) => t.id === id)?.name;
		return `the ${teamName ?? ''} team`;
	}
	return undefined;
}

export function conversationSubtitle(c: Conversation, snapshot: OrgSnapshot): string {
	const count = `${c.members.length} ${c.members.length === 1 ? 'member' : 'members'}`;
	if (!c.auto) return count;
	return `${count} · follows ${followsWords(c, snapshot)}`;
}

export function copyFreshness(copyAt: string | undefined, now: string): string | undefined {
	if (!copyAt) return undefined;
	const diff = Date.parse(now) - Date.parse(copyAt);
	if (Number.isNaN(diff) || diff < 0) return undefined;
	const age = span(diff);
	return age ? `Last updated ${age} ago.` : undefined;
}

export function isTextInputTarget(field: EventTarget | null): boolean {
	if (typeof HTMLTextAreaElement !== 'undefined' && field instanceof HTMLTextAreaElement)
		return field.value !== '';
	if (typeof HTMLInputElement !== 'undefined' && field instanceof HTMLInputElement) {
		return /^(text|search|email|url|tel|password|number)$/.test(field.type) && field.value !== '';
	}
	return false;
}

export function pendingToMessage(p: PendingMessage, ownerId: string, now: string): ChatMessage {
	return {
		id: `pending-${p.key}`,
		conversation_id: p.conversation_id,
		thread_root_id: p.thread_root_id,
		also_in_channel: p.also_in_channel,
		from: ownerId,
		kind: 'msg',
		body: p.body,
		created_at: now,
		attachments: p.attachments
	};
}

export function dayChanged(list: ChatMessage[], index: number, now: string): boolean {
	return (
		index === 0 ||
		dayLabel(list[index].created_at, now) !== dayLabel(list[index - 1].created_at, now)
	);
}
