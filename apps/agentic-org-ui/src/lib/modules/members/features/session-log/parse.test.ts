import { describe, expect, it } from 'vitest';
import { touchedFiles, usageTotals, visibleEntries } from './entries';
import { parseClaude } from './parse-claude';
import { filesFromPatch, parseCodex } from './parse-codex';

describe('parseClaude', () => {
	const lines = [
		{
			type: 'user',
			timestamp: 't0',
			version: '2.1.4',
			message: { role: 'user', content: 'Fix it' }
		},
		{
			type: 'assistant',
			timestamp: 't1',
			message: {
				role: 'assistant',
				content: [
					{ type: 'thinking', thinking: 'x' },
					{ type: 'text', text: 'Looking' },
					{
						type: 'tool_use',
						id: 'u1',
						name: 'Edit',
						input: { file_path: 'a.ts', old_string: 'a', new_string: 'b\nc' }
					}
				],
				usage: { input_tokens: 10, output_tokens: 2 }
			}
		},
		{
			type: 'user',
			message: {
				role: 'user',
				content: [{ type: 'tool_result', tool_use_id: 'u1', is_error: true, content: 'nope' }]
			}
		},
		{ type: 'progress', data: {} },
		'not an object'
	];
	const { entries, toolVersion, startedAt } = parseClaude(lines);

	it('maps messages, tool calls, files, results and usage, and keeps unknown lines raw', () => {
		expect(entries.map((e) => e.kind)).toEqual([
			'message',
			'message',
			'tool_call',
			'file',
			'usage',
			'tool_result',
			'raw',
			'raw'
		]);
		expect(toolVersion).toBe('2.1.4');
		expect(startedAt).toBe('t0');
	});
	it('turns an Edit into a removed/added diff', () => {
		const file = entries.find((e) => e.kind === 'file');
		expect(file).toMatchObject({
			path: 'a.ts',
			action: 'edit',
			diff: [
				{ op: 'remove', text: 'a' },
				{ op: 'add', text: 'b' },
				{ op: 'add', text: 'c' }
			]
		});
	});
	it('marks a failed tool result', () => {
		expect(entries.find((e) => e.kind === 'tool_result')).toMatchObject({
			ok: false,
			output: 'nope',
			callId: 'u1'
		});
	});
});

describe('parseCodex', () => {
	const patch =
		'*** Begin Patch\n*** Update File: s.sh\n@@\n-old\n+new\n*** Add File: t.sh\n+x\n*** End Patch';
	const lines = [
		{ timestamp: 't0', type: 'session_meta', payload: { cli_version: '0.58.0', cwd: '/w' } },
		{
			timestamp: 't1',
			type: 'response_item',
			payload: { type: 'message', role: 'user', content: [{ type: 'input_text', text: 'Go' }] }
		},
		{
			timestamp: 't2',
			type: 'response_item',
			payload: {
				type: 'function_call',
				name: 'shell',
				arguments: '{"command":["bash","-lc","ls"]}',
				call_id: 'c1'
			}
		},
		{
			timestamp: 't3',
			type: 'response_item',
			payload: {
				type: 'function_call_output',
				call_id: 'c1',
				output: '{"output":"x","metadata":{"exit_code":2}}'
			}
		},
		{
			timestamp: 't4',
			type: 'response_item',
			payload: { type: 'custom_tool_call', name: 'apply_patch', call_id: 'c2', input: patch }
		},
		{ timestamp: 't5', type: 'event_msg', payload: { type: 'token_count', info: null } },
		{ timestamp: 't6', type: 'compacted', payload: {} }
	];
	const { entries, toolVersion } = parseCodex(lines);

	it('maps the rollout to the same entry model', () => {
		expect(entries.map((e) => e.kind)).toEqual([
			'notice',
			'message',
			'tool_call',
			'tool_result',
			'tool_call',
			'file',
			'file',
			'usage',
			'raw'
		]);
		expect(toolVersion).toBe('0.58.0');
	});
	it('reads a non-zero exit code as a failed result', () => {
		expect(entries.find((e) => e.kind === 'tool_result')).toMatchObject({ ok: false, output: 'x' });
	});
	it('marks missing token counts incomplete rather than zero', () => {
		expect(entries.find((e) => e.kind === 'usage')).toMatchObject({ complete: false });
		expect(usageTotals(entries)?.complete).toBe(false);
	});
	it('splits a patch per file', () => {
		expect(filesFromPatch(patch).map((f) => [f.path, f.action])).toEqual([
			['s.sh', 'edit'],
			['t.sh', 'create']
		]);
	});
});

describe('toggles and touched files', () => {
	const { entries } = parseCodex([
		{
			type: 'response_item',
			payload: {
				type: 'custom_tool_call',
				name: 'apply_patch',
				input: '*** Update File: a\n-x\n+y\n+z'
			}
		},
		{
			type: 'response_item',
			payload: { type: 'message', role: 'assistant', content: [{ text: 'ok' }] }
		}
	]);
	it('hides tool calls and file entries independently', () => {
		expect(visibleEntries(entries, { tools: false, files: true }).map((e) => e.kind)).toEqual([
			'file',
			'message'
		]);
		expect(visibleEntries(entries, { tools: true, files: false }).map((e) => e.kind)).toEqual([
			'tool_call',
			'message'
		]);
	});
	it('counts each file once with its added and removed lines', () => {
		expect(touchedFiles(entries)).toEqual([
			{ path: 'a', action: 'edit', added: 2, removed: 1, entryIds: ['l0.f0'] }
		]);
	});
});
