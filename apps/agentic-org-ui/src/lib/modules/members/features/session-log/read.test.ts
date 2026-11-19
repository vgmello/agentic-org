import { describe, expect, it } from 'vitest';
import { formatName, parseSessionFile, previewLines, readSessionLog } from './read';

const claudeLines = [
	{
		type: 'user',
		timestamp: '2026-11-18T09:00:00Z',
		version: '2.1.4',
		message: { role: 'user', content: 'Fix it' }
	}
];

describe('session log reading', () => {
	it('parses a Claude file with the Claude parser and keeps the run and member', () => {
		const log = parseSessionFile({
			runId: 'r1',
			memberId: 'm1',
			format: 'claude',
			lines: claudeLines
		});
		expect(log).toMatchObject({
			runId: 'r1',
			memberId: 'm1',
			format: 'claude',
			toolVersion: '2.1.4'
		});
		expect(log.entries[0]).toMatchObject({ kind: 'message', role: 'user', text: 'Fix it' });
	});

	it('parses a Codex file with the Codex parser; an unknown line stays as a raw entry', () => {
		const log = parseSessionFile({
			runId: 'r2',
			memberId: 'm2',
			format: 'codex',
			lines: [{ type: 'something_new', payload: 1 }]
		});
		expect(log.format).toBe('codex');
		expect(log.entries.some((e) => e.kind === 'raw')).toBe(true);
	});

	it('reads through the api and passes its refusal on', async () => {
		const api = {
			sessionFile: async (_o: string, memberId: string, runId: string) => ({
				runId,
				memberId,
				format: 'claude' as const,
				lines: claudeLines
			})
		};
		await expect(readSessionLog(api, 'o', 'm', 'r')).resolves.toMatchObject({ runId: 'r' });
		const refusing = { sessionFile: () => Promise.reject({ code: 'not_found' }) };
		await expect(readSessionLog(refusing, 'o', 'm', 'r')).rejects.toMatchObject({
			code: 'not_found'
		});
	});

	it('names the runtime with its version', () => {
		expect(formatName({ format: 'claude', toolVersion: '2.1.4' })).toBe('Claude Code 2.1.4');
		expect(formatName({ format: 'codex' })).toBe('Codex');
	});

	it('cuts long output and counts what is hidden', () => {
		expect(previewLines('a\nb\nc', 2)).toEqual({ shown: 'a\nb', hidden: 1 });
		expect(previewLines('a\nb', 2)).toEqual({ shown: 'a\nb', hidden: 0 });
	});
});
