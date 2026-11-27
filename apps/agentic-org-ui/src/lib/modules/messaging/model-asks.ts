// Asks, hand-offs, browse channels, pins and collapse helpers (CHAT-5, CHAT-6, CHAT-14, CHAT-17, CHAT-18).
import { span } from '$lib/modules/work/time';
import { answerText, isMember, optionsOf, questionsOf, type ChatContext } from './engine';
import { clockTime, ms, nameOf, whoOf } from './model-sidebar';
import { CHAT_LIMITS, type ChatMessage, type ChatState, type Conversation } from './types';

export const waitingFor = (m: ChatMessage, now: string): string =>
	`Waiting ${span(Math.max(0, ms(now) - ms(m.created_at)))}`;

export interface DeadlineWords {
	text: string;
	applied: boolean;
}

function defaultSummaryOf(m: ChatMessage): string | undefined {
	if (m.default_option) {
		return m.options?.find((o) => o.id === m.default_option)?.label ?? m.default_option;
	}
	const labels = m.questions
		?.filter((q) => q.default_option)
		.map((q) => optionsOf(q).find((o) => o.id === q.default_option)?.label ?? q.default_option!);
	return labels?.length ? labels.join(', ') : undefined;
}

export function deadlineWords(
	m: ChatMessage,
	now: string,
	clock: (iso: string) => string = clockTime
): DeadlineWords | undefined {
	if (m.kind !== 'request' || !m.default_at) return undefined;
	const label = defaultSummaryOf(m);
	if (!label) return undefined;
	if (m.answer?.by === 'default')
		return {
			text: `No answer by ${clock(m.default_at)}: the default was applied (${label}).`,
			applied: true
		};
	if (m.answer) return undefined;
	if (ms(m.default_at) <= ms(now))
		return {
			text: `The deadline passed: the default (${label}) is being applied.`,
			applied: false
		};
	return {
		text: `If no answer by ${clock(m.default_at)}, the default applies: ${label}. (${span(ms(m.default_at) - ms(now))} left)`,
		applied: false
	};
}

export function acceptedWords(
	m: ChatMessage,
	ctx: ChatContext,
	clock: (iso: string) => string = clockTime
): string | undefined {
	return m.kind === 'handoff' && m.accepted_by
		? `Accepted by ${nameOf(ctx, m.accepted_by)}${m.accepted_at ? ` · ${clock(m.accepted_at)}` : ''}`
		: undefined;
}

export function answerWords(m: ChatMessage, ctx: ChatContext): string | undefined {
	if (!m.answer) return undefined;
	const label = m.options?.find((o) => o.id === m.answer!.option_id)?.label.replace(/…$/, '');
	const by = m.answer.by === 'default' ? 'the default' : nameOf(ctx, m.answer.by);
	return `Answered by ${by}${label ? `: ${label}` : ''}${m.answer.text ? `${label ? ' · ' : ': '}${m.answer.text}` : ''}`;
}

export const appOnly = (m: ChatMessage, ctx: ChatContext): boolean =>
	(m.kind === 'request' && (m.addressed_to === ctx.owner || !!m.approval)) ||
	(m.kind === 'handoff' && m.addressed_to === ctx.owner);

export function outcomeWords(
	m: ChatMessage,
	ctx: ChatContext,
	clock: (iso: string) => string = clockTime
): string | undefined {
	if (m.withdrawn) {
		const reason = m.withdrawn.reason ? `: ${m.withdrawn.reason}` : '';
		return `Withdrawn by ${whoOf(ctx, m.withdrawn.by)} · ${clock(m.withdrawn.at)}${reason}`;
	}
	if (!m.answer) return undefined;
	const questions = questionsOf(m);
	const said = m.answer.answers
		? questions.length === 1
			? answerText(questions[0], m.answer.answers[0])
			: `${m.answer.answers.length} of ${questions.length} answered`
		: [m.options?.find((o) => o.id === m.answer!.option_id)?.label.replace(/…$/, ''), m.answer.text]
				.filter(Boolean)
				.join(': ');
	const suffix = said ? `: ${said}` : '';
	if (m.answer.by === 'default') return `Default applied · ${clock(m.answer.at)}${suffix}`;
	return `Answered by ${whoOf(ctx, m.answer.by)} · ${clock(m.answer.at)}${suffix}`;
}

export function handoffOutcome(
	m: ChatMessage,
	ctx: ChatContext,
	clock: (iso: string) => string = clockTime
): string | undefined {
	if (m.kind !== 'handoff') return undefined;
	if (m.accepted_by)
		return `Accepted by ${whoOf(ctx, m.accepted_by)}${m.accepted_at ? ` · ${clock(m.accepted_at)}` : ''}`;
	if (m.handed_back)
		return `Handed back by ${whoOf(ctx, m.handed_back.by)} · ${clock(m.handed_back.at)}: ${m.handed_back.reason}`;
	return undefined;
}

export function requiredWords(m: ChatMessage, ctx: ChatContext, now: string): string | undefined {
	if (m.kind !== 'request' || !m.required || m.answer || m.withdrawn) return undefined;
	const asker = nameOf(ctx, m.from);
	if (m.wait_until && ms(m.wait_until) <= ms(now))
		return `Required · ${asker} is stopped, waiting for this answer`;
	return `Required · ${asker} can't continue without this answer`;
}

export interface BrowseRow {
	conversation: Conversation;
	scope?: string;
	member: boolean;
	action: 'open' | 'join' | 'request' | 'requested' | 'none';
}

function browseScope(
	c: Conversation,
	projects: { id: string; name: string }[],
	teams: { id: string; name: string }[]
): string | undefined {
	if (!c.auto) return undefined;
	if (c.auto.type === 'general') return 'Everyone';
	if (c.auto.type === 'project') {
		const id = c.auto.project_id;
		return `${projects.find((p) => p.id === id)?.name ?? 'A'} project`;
	}
	const id = c.auto.team_id;
	return `${teams.find((t) => t.id === id)?.name ?? 'A'} team`;
}

function browseAction(c: Conversation, viewer: string, member: boolean): BrowseRow['action'] {
	if (member) return 'open';
	if (c.auto) return 'none';
	if (c.access === 'invite') return c.invite_requests?.includes(viewer) ? 'requested' : 'request';
	return 'join';
}

export function browseChannels(
	s: ChatState,
	ctx: ChatContext,
	projects: { id: string; name: string }[] = [],
	teams: { id: string; name: string }[] = []
): BrowseRow[] {
	const viewer = ctx.actor;
	return s.conversations
		.filter(
			(c) =>
				c.kind === 'channel' && !c.archived && (isMember(c, viewer) || c.access !== 'restricted')
		)
		.map((c) => {
			const member = isMember(c, viewer);
			const scope = browseScope(c, projects, teams);
			return {
				conversation: c,
				...(scope ? { scope } : {}),
				member,
				action: browseAction(c, viewer, member)
			};
		})
		.sort((a, b) => (a.conversation.name ?? '').localeCompare(b.conversation.name ?? ''));
}

export const pinsIn = (s: ChatState, conversationId: string): ChatMessage[] =>
	s.messages
		.filter((m) => m.conversation_id === conversationId && m.pinned)
		.sort((a, b) => ms(b.created_at) - ms(a.created_at));

export function collapse(
	body: string,
	limit: number = CHAT_LIMITS.collapseLines
): { shown: string; hidden: number } {
	const lines = body.split('\n');
	return lines.length <= limit
		? { shown: body, hidden: 0 }
		: { shown: lines.slice(0, limit).join('\n'), hidden: lines.length - limit };
}
