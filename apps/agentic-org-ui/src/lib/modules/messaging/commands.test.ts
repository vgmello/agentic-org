import { describe, expect, it } from 'vitest';
import { commandForm, turnForm, type CommandEnv } from './commands';
import { parseSlash } from './model';
import { byId, ctxAs, fresh, kaiAsk } from './testkit';

const ctx = ctxAs();
const env: CommandEnv = { projects: [{ id: 'atlas', name: 'Atlas' }, { id: 'beacon', name: 'Beacon' }], members: ctx.members, conversationTitle: '#atlas' };
const form = (text: string) => {
	const p = parseSlash(text);
	if (!p || !p.ok) throw new Error('not a command');
	return commandForm(p, env);
};

describe('CHAT-9 · the forms behind the slash commands', () => {
	it('CHAT-9 · /bug opens the bug form prefilled from the text', () => {
		const f = form('/bug the list is empty');
		expect(f).toMatchObject({ command: 'bug', submit: 'Report bug' });
		expect('fields' in f && f.fields.map((x) => [x.id, x.value])).toEqual([['title', 'the list is empty'], ['happened', ''], ['project', 'atlas']]);
		expect('done' in f && f.done({ title: 'It breaks', project: 'beacon' })).toBe('Bug "It breaks" drafted for Beacon. Sample data: nothing was saved.');
	});
	it('CHAT-9 · /request opens the request form, and says it is not a message kind', () => {
		const f = form('/request dark mode');
		expect(f).toMatchObject({ command: 'request' });
		expect('description' in f && f.description).toContain('REQ-n');
	});
	it('CHAT-9 · /task @agent assigns to that agent', () => {
		const f = form('/task @sam write the docs');
		expect(f).toMatchObject({ command: 'task', title: 'Assign a task to Sam Okafor' });
		expect('fields' in f && f.fields.find((x) => x.id === 'assignee')?.value).toBe('sam');
		expect(form('/task @nobody write')).toEqual({ error: 'There is no agent or person @nobody.' });
	});
	it('CHAT-9 · /status picks the project named', () => {
		expect('fields' in form('/status beacon') && (form('/status beacon') as { fields: { value: string }[] }).fields[0].value).toBe('beacon');
		expect(form('/status')).toMatchObject({ command: 'status', submit: 'Post status card' });
	});
	it('CHAT-9 · /wake runs an agent now, and only an agent', () => {
		expect(form('/wake @ines')).toMatchObject({ command: 'wake', title: 'Run Inês Duarte now' });
		expect(form('/wake @avery-stone')).toEqual({ error: 'There is no agent @avery-stone.' });
	});
	it('CHAT-9 · /remind reads "in 2h" and offers the usual times', () => {
		const f = form('/remind in 2h check the build');
		expect(f).toMatchObject({ command: 'remind' });
		const fields = (f as { fields: { id: string; value: string }[] }).fields;
		expect(fields.find((x) => x.id === 'when')?.value).toBe('7200000');
		expect(fields.find((x) => x.id === 'what')?.value).toBe('check the build');
		expect((f as { done: (v: Record<string, string>) => string }).done({ what: 'check the build' })).toBe('Reminder set for #atlas: check the build. Sample data: nothing was saved.');
		expect((form('/remind in 90m x') as { fields: { options: { label: string }[] }[] }).fields[0].options.at(-1)?.label).toBe('in 90 minutes');
	});
	it('CHAT-9 · Turn into… fills the same forms from a message', () => {
		const m = byId(fresh(), kaiAsk(fresh()).id);
		const bug = turnForm('bug', m, env);
		expect(bug.fields.find((x) => x.id === 'title')?.value).toBe('Block bug reports when a project has no team?');
		expect(bug.fields.find((x) => x.id === 'happened')?.value).toBe(m.body);
		expect(turnForm('task', m, env).command).toBe('task');
		expect(turnForm('request', m, env).command).toBe('request');
	});
});
