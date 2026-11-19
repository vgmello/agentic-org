// The forms behind the slash commands and "Turn into…" (CHAT-9). UI only: they show what the command would send and
// confirm in words. Nothing is saved until the office serves these calls (`bug.report`, `task.create`, requests, status).
import type { ChatMember } from './engine';
import { remindDelay, turnInto, type Slash } from './model';
import type { ChatMessage } from './types';

export interface CommandField {
	id: string;
	label: string;
	kind: 'text' | 'textarea' | 'select';
	value: string;
	options?: { value: string; label: string }[];
	required?: boolean;
}

export interface CommandForm {
	command: 'bug' | 'request' | 'task' | 'status' | 'wake' | 'remind';
	title: string;
	description: string;
	fields: CommandField[];
	submit: string;
	/** What the toast says after Submit, from the field values. */
	done: (values: Record<string, string>) => string;
}

export interface CommandEnv {
	projects: { id: string; name: string }[];
	members: ChatMember[];
	/** The conversation the command was typed in, for the reminder. */
	conversationTitle: string;
}

const NOT_SAVED = 'Sample data: nothing was saved.';
const project = (env: CommandEnv, value = ''): CommandField => ({ id: 'project', label: 'Project', kind: 'select', value: value || env.projects[0]?.id || '', options: env.projects.map((p) => ({ value: p.id, label: p.name })), required: true });
const projectName = (env: CommandEnv, id: string) => env.projects.find((p) => p.id === id)?.name ?? 'the project';
const person = (env: CommandEnv, handle: string | undefined) => env.members.find((m) => m.handle.toLowerCase() === handle);

/** The form for a parsed slash command, or the sentence to show when it can't open one. */
export function commandForm(cmd: Extract<Slash, { ok: true }>, env: CommandEnv): CommandForm | { error: string } {
	switch (cmd.name) {
		case 'bug':
			return bugForm(env, cmd.rest, '');
		case 'request':
			return requestForm(env, cmd.rest, '');
		case 'task': {
			const who = person(env, cmd.handle);
			if (!who || who.former) return { error: `There is no agent or person @${cmd.handle}.` };
			return taskForm(env, cmd.rest, '', who);
		}
		case 'status':
			return {
				command: 'status',
				title: 'Post the project status',
				description: 'Posts the live status card here (project-status-ux.md). The card comes with the status feature.',
				fields: [project(env, env.projects.find((p) => cmd.rest && p.name.toLowerCase().includes(cmd.rest.toLowerCase()))?.id)],
				submit: 'Post status card',
				done: (v) => `The status card for ${projectName(env, v.project)} would be posted here. ${NOT_SAVED}`
			};
		case 'wake': {
			const who = person(env, cmd.handle);
			if (!who || who.kind !== 'agent' || who.former) return { error: `There is no agent @${cmd.handle}.` };
			return {
				command: 'wake',
				title: `Run ${who.name} now`,
				description: `${who.name} starts a session now instead of waiting for its next run.`,
				fields: [],
				submit: 'Run now',
				done: () => `${who.name} would be woken now. ${NOT_SAVED}`
			};
		}
		case 'remind': {
			const r = remindDelay(cmd.rest);
			return {
				command: 'remind',
				title: 'Remind me',
				description: `About ${env.conversationTitle}.`,
				fields: [
					{ id: 'when', label: 'When', kind: 'select', value: r ? String(r.ms) : '3600000', options: [['in 30 minutes', 1_800_000], ['in 1 hour', 3_600_000], ['in 2 hours', 7_200_000], ['tomorrow', 86_400_000]].map(([label, ms]) => ({ value: String(ms), label: String(label) })).concat(r && ![1_800_000, 3_600_000, 7_200_000, 86_400_000].includes(r.ms) ? [{ value: String(r.ms), label: `in ${r.ms / 60_000} minutes` }] : []) },
					{ id: 'what', label: 'About', kind: 'text', value: r ? r.what : cmd.rest }
				],
				submit: 'Set reminder',
				done: (v) => `Reminder set for ${env.conversationTitle}${v.what ? `: ${v.what}` : ''}. ${NOT_SAVED}`
			};
		}
	}
}

/** Turn into… on a message's ⋯: the same forms, filled from the message. */
export function turnForm(into: 'bug' | 'request' | 'task', m: ChatMessage, env: CommandEnv): CommandForm {
	const { title, body } = turnInto(m, into);
	if (into === 'bug') return bugForm(env, title, body);
	if (into === 'request') return requestForm(env, title, body);
	return taskForm(env, title, body, undefined);
}

function bugForm(env: CommandEnv, title: string, body: string): CommandForm {
	return {
		command: 'bug',
		title: 'Report a bug',
		description: 'Prefilled from chat. You can change anything before you send it.',
		fields: [
			{ id: 'title', label: 'Title', kind: 'text', value: title, required: true },
			{ id: 'happened', label: 'What happened', kind: 'textarea', value: body, required: true },
			project(env)
		],
		submit: 'Report bug',
		done: (v) => `Bug "${v.title}" drafted for ${projectName(env, v.project)}. ${NOT_SAVED}`
	};
}

function requestForm(env: CommandEnv, title: string, body: string): CommandForm {
	return {
		command: 'request',
		title: 'Send a feature request',
		description: 'A request is its own thing (REQ-n), not a chat message.',
		fields: [
			{ id: 'title', label: 'What do you need', kind: 'text', value: title, required: true },
			{ id: 'why', label: 'Why', kind: 'textarea', value: body },
			project(env)
		],
		submit: 'Send request',
		done: (v) => `Request "${v.title}" drafted for ${projectName(env, v.project)}. ${NOT_SAVED}`
	};
}

function taskForm(env: CommandEnv, title: string, body: string, who: ChatMember | undefined): CommandForm {
	return {
		command: 'task',
		title: who ? `Assign a task to ${who.name}` : 'New task',
		description: 'It starts in To do.',
		fields: [
			{ id: 'title', label: 'Title', kind: 'text', value: title, required: true },
			{ id: 'description', label: 'Description', kind: 'textarea', value: body },
			{ id: 'assignee', label: 'Assignee', kind: 'select', value: who?.id ?? '', options: [{ value: '', label: 'Unassigned' }, ...env.members.filter((m) => !m.former && !m.owner).map((m) => ({ value: m.id, label: m.name }))] },
			project(env)
		],
		submit: 'Create task',
		done: (v) => `Task "${v.title}" drafted for ${projectName(env, v.project)}. ${NOT_SAVED}`
	};
}
