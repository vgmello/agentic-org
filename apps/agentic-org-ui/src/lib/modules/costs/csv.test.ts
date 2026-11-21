import { describe, expect, it } from 'vitest';
import { USAGE_CSV_HEADERS, buildUsageCsv, escapeCsvCell } from './csv';
import type { UsageRecord } from './types';

describe('CSV formula-injection escaping and RFC 4180 encoding', () => {
	it.each([
		['=SUM(A1:A2)', "'=SUM(A1:A2)"],
		['+cmd|"/C calc"!A0', `"'""+cmd|""/C calc""!A0"`.replace(`"'""`, `"'`)],
		['-2+3', "'-2+3"],
		['@SUM(1+1)', "'@SUM(1+1)"],
		['\t=1+1', "'\t=1+1"],
		['\r=1+1', `"'\r=1+1"`],
		['   =1+1', "'   =1+1"]
	])('neutralizes leading formula trigger %j', (input, expected) => {
		expect(escapeCsvCell(input)).toBe(expected);
	});

	it('quotes commas, quotes, and newlines per RFC 4180 without corrupting safe strings', () => {
		expect(escapeCsvCell('Atlas')).toBe('Atlas');
		expect(escapeCsvCell('Fix "pay", retry')).toBe('"Fix ""pay"", retry"');
		expect(escapeCsvCell('line1\nline2')).toBe('"line1\nline2"');
		expect(escapeCsvCell(null)).toBe('');
		expect(escapeCsvCell(undefined)).toBe('');
		expect(escapeCsvCell(true)).toBe('true');
		expect(escapeCsvCell(4200)).toBe('4200');
		expect(escapeCsvCell(-5)).toBe(`"'-5"`);
	});

	it('builds one row per effective usage record with lineage, exact USD, and checkpoint', () => {
		const record: UsageRecord = {
			id: '0193b000-0000-7000-8000-000000000001',
			occurredAt: '2026-11-18T10:00:00Z',
			consumer: { kind: 'agent', memberId: 'sam', name: 'Sam Okafor' },
			runtime: 'claude_code',
			provider: 'anthropic',
			model: 'claude-sonnet-4-5',
			tokens: { input: 1000, output: 200, cacheRead: 500, cacheWrite: 50, reasoning: 40 },
			tokenStatus: 'complete',
			costMicroUsd: 1_234_567,
			costBasis: 'runtime_estimate',
			costStatus: 'complete',
			apiEquivalent: true,
			price: { id: 'p-1', label: 'anthropic/claude-sonnet-4-5' },
			attribution: {
				basis: 'work_for',
				task: { ref: 'T-12', title: '=HYPERLINK("http://bad","Click")' },
				project: { id: 'atlas', name: 'Atlas' },
				team: { id: 'platform', name: 'Platform' }
			},
			workType: 'build',
			correction: {
				byName: 'Alex Kim',
				at: '2026-11-18T12:00:00Z',
				reason: '+Moved from overhead'
			}
		};
		const csv = buildUsageCsv([record], {
			checkpointAt: '2026-11-19T14:58:00Z',
			gaps: []
		});
		const lines = csv.split('\r\n');
		expect(lines[0]).toBe(USAGE_CSV_HEADERS.join(','));
		expect(lines[1]).toContain('0193b000-0000-7000-8000-000000000001');
		expect(lines[1]).toContain('1.234567');
		expect(lines[1]).toContain(`"'=HYPERLINK(""http://bad"",""Click"")"`);
		expect(lines[1]).toContain("'+Moved from overhead");
		expect(lines[1]).toContain('2026-11-19T14:58:00Z');
		expect(lines[2]).toBe('');
	});
});
