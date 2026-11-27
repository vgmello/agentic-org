// Composer state, mentions, slash commands and turn-into helpers (CHAT-7, CHAT-9, CHAT-14).
import { isMember, memberOf, mentionsIn, type ChatContext, type ChatMember } from './engine';
import { plain } from './markdown';
import { nameOf } from './model-sidebar';
import type { ChatMessage, ChatState, Conversation } from './types';

export const foldText = (t: string) =>
	t
		.normalize('NFKD')
		.replace(/[\u0300-\u036f]/g, '')
		.toLowerCase();

export interface ComposerState {
	/** Whether the owner can write here. */
	canWrite: boolean;
	/** The line under or instead of the box. */
	note?: string;
	/** Agents this message would wake. */
	wakes: string[];
}

export function cannotRunWords(ctx: ChatContext, id: string): string {
	const name = nameOf(ctx, id);
	return memberOf(ctx, id)?.handle === 'hr'
		? `${name} can't run right now, so it won't answer yet. To hire, use Hire and fill in the details yourself.`
		: `${name} can't run right now, so it won't answer yet.`;
}

export function cannotRunFor(
	ctx: ChatContext,
	c: Conversation,
	body: string,
	cannotRun: readonly string[] = []
): string[] {
	const reached =
		c.kind === 'dm' ? c.members.filter((id) => id !== ctx.actor) : mentionsIn(body, ctx);
	return reached.filter((id) => cannotRun.includes(id));
}

export function composerState(
	s: ChatState,
	ctx: ChatContext,
	c: Conversation,
	officeRunning: boolean,
	cannotRun: readonly string[] = []
): ComposerState {
	if (!officeRunning)
		return {
			canWrite: false,
			note: "The office isn't running. Restart it from Offices.",
			wakes: []
		};
	if (c.archived)
		return {
			canWrite: false,
			note: `${c.kind === 'channel' ? `#${c.name}` : 'This conversation'} is archived.`,
			wakes: []
		};
	const gone =
		c.kind === 'dm' ? c.members.map((id) => memberOf(ctx, id)).find((m) => m?.former) : undefined;
	if (gone) return { canWrite: false, note: `${gone.name} was offboarded.`, wakes: [] };
	if (!isMember(c, ctx.owner)) {
		const agent = c.members.map((id) => memberOf(ctx, id)).find((m) => m?.kind === 'agent');
		return {
			canWrite: false,
			note: `Reading as the owner. To weigh in, message ${agent?.name ?? 'a member'}.`,
			wakes: []
		};
	}
	const others = c.kind === 'dm' ? c.members.filter((id) => id !== ctx.owner) : [];
	const blocked = cannotRunFor(ctx, c, '', cannotRun);
	if (blocked.length) return { canWrite: true, note: cannotRunWords(ctx, blocked[0]), wakes: [] };
	const paused = others.filter((id) => s.presence[id] === 'paused');
	if (paused.length)
		return {
			canWrite: true,
			note: `${paused.map((id) => nameOf(ctx, id)).join(' and ')} ${paused.length === 1 ? 'is' : 'are'} paused and will not read this until resumed.`,
			wakes: []
		};
	return { canWrite: true, wakes: [] };
}

export function wakesFor(s: ChatState, ctx: ChatContext, c: Conversation, body: string): string[] {
	if (c.kind === 'dm')
		return c.members.filter((id) => id !== ctx.owner && s.presence[id] === 'asleep');
	return mentionsIn(body, ctx).filter(
		(id) => memberOf(ctx, id)?.kind === 'agent' && s.presence[id] === 'asleep'
	);
}

export interface MentionQuery {
	start: number;
	query: string;
}

export function mentionAt(text: string, caret: number): MentionQuery | undefined {
	const m = /(^|[^\w@])@([\w-]*)$/.exec(text.slice(0, caret));
	return m ? { start: caret - m[2].length - 1, query: m[2] } : undefined;
}

export interface MentionOption {
	handle: string;
	name: string;
	kind: 'agent' | 'person' | 'here';
}

export function mentionOptions(ctx: ChatContext, c: Conversation, query: string): MentionOption[] {
	const q = foldText(query);
	const pool =
		c.kind === 'channel' || c.kind === 'group_dm' ? c.members : ctx.members.map((m) => m.id);
	const members = pool
		.map((id) => memberOf(ctx, id))
		.filter((m): m is ChatMember => !!m && !m.former && !m.owner);
	const hits = members.filter(
		(m) =>
			q === '' ||
			foldText(m.name)
				.split(/\s+/)
				.some((w) => w.startsWith(q)) ||
			foldText(m.handle).startsWith(q) ||
			foldText(m.name).startsWith(q)
	);
	const out: MentionOption[] = hits
		.sort((a, b) => a.name.localeCompare(b.name))
		.map((m) => ({ handle: m.handle, name: m.name, kind: m.kind }));
	if (c.kind === 'channel' && 'here'.startsWith(q))
		out.unshift({ handle: 'here', name: "Everyone in this channel's members", kind: 'here' });
	return out.slice(0, 8);
}

export function applyMention(
	text: string,
	at: MentionQuery,
	caret: number,
	handle: string
): { text: string; caret: number } {
	const inserted = `@${handle} `;
	return {
		text: text.slice(0, at.start) + inserted + text.slice(caret),
		caret: at.start + inserted.length
	};
}

export type SlashName = 'bug' | 'request' | 'task' | 'status' | 'wake' | 'remind';
export const SLASH_COMMANDS: { name: SlashName; usage: string; help: string }[] = [
	{ name: 'bug', usage: '/bug [what happened]', help: 'Report a bug, prefilled from your text' },
	{ name: 'request', usage: '/request [what you need]', help: 'Send a feature request' },
	{ name: 'task', usage: '/task @agent [title]', help: 'Assign a task' },
	{ name: 'status', usage: '/status [project]', help: 'Post the live status card' },
	{ name: 'wake', usage: '/wake @agent', help: 'Run an agent now' },
	{ name: 'remind', usage: '/remind [in 30m] [what]', help: 'Remind you about this conversation' }
];

export type Slash =
	| { ok: true; name: SlashName; args: string; handle?: string; rest: string }
	| { ok: false; message: string; unknown?: true };

function editDistance(a: string, b: string): number {
	const row = Array.from({ length: b.length + 1 }, (_, j) => j);
	for (let i = 1; i <= a.length; i++) {
		let prev = row[0];
		row[0] = i;
		for (let j = 1; j <= b.length; j++) {
			const keep = row[j];
			row[j] = Math.min(row[j] + 1, row[j - 1] + 1, prev + (a[i - 1] === b[j - 1] ? 0 : 1));
			prev = keep;
		}
	}
	return row[b.length];
}

export function parseSlash(text: string): Slash | undefined {
	const m = /^\/([a-z0-9-]+)(?:\s+([\s\S]*))?$/i.exec(text);
	if (!m) return undefined;
	const name = m[1].toLowerCase();
	const cmd = SLASH_COMMANDS.find((c) => c.name === name);
	if (!cmd) {
		const sorted = (t: string) => [...t].sort().join('');
		const near = SLASH_COMMANDS.find(
			(c) => editDistance(c.name, name) <= 1 || sorted(c.name) === sorted(name)
		);
		return {
			ok: false,
			unknown: true,
			message: `No command /${m[1]}.${near ? ` Did you mean /${near.name}?` : ''}`
		};
	}
	const args = (m[2] ?? '').trim();
	const who = /^@([\w-]+)\s*([\s\S]*)$/.exec(args);
	if ((cmd.name === 'task' || cmd.name === 'wake') && !who)
		return { ok: false, message: `Say which agent: ${cmd.usage}` };
	if (cmd.name === 'task' && !who![2].trim())
		return { ok: false, message: 'Give the task a title: /task @agent [title]' };
	return {
		ok: true,
		name: cmd.name,
		args,
		...(who ? { handle: who[1].toLowerCase(), rest: who[2].trim() } : { rest: args })
	};
}

export const slashSuggestions = (text: string) => {
	const m = /^\/([a-z]*)$/i.exec(text);
	return m ? SLASH_COMMANDS.filter((c) => c.name.startsWith(m[1].toLowerCase())) : [];
};

export function remindDelay(text: string): { ms: number; what: string } | undefined {
	const m = /^in\s+(\d+)\s*(m|h|d)\b\s*([\s\S]*)$/i.exec(text.trim());
	if (!m) return undefined;
	const n = Number(m[1]);
	return {
		ms: n * { m: 60_000, h: 3_600_000, d: 86_400_000 }[m[2].toLowerCase() as 'm'],
		what: m[3].trim()
	};
}

export function turnInto(
	m: ChatMessage,
	into: 'bug' | 'request' | 'task'
): { title: string; body: string } {
	const text = plain(m.body);
	const first = text.split(/(?<=[.?!])\s/)[0] ?? text;
	const title = (first.length > 80 ? first.slice(0, 79).trimEnd() + '…' : first)
		.replace(/^(@[\w-]+\s*)+/, '')
		.trim();
	return {
		title:
			title ||
			(into === 'bug' ? 'Bug from chat' : into === 'task' ? 'Task from chat' : 'Request from chat'),
		body: m.body
	};
}
