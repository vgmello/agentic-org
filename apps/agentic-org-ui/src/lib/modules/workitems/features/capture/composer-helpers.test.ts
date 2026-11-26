import { describe, expect, it } from 'vitest';
import type { EmployeeChoice } from '../../types';
import {
	applyMentionReplacement,
	buildTessDraftPrompt,
	detectMentionMatch,
	filterMentionCandidates
} from './composer-helpers';

const EMPLOYEES: EmployeeChoice[] = [
	{
		id: 'emp-1',
		name: 'Kai Chen',
		kind: 'agent',
		role: 'Backend Engineer',
		availability: 'Available'
	},
	{
		id: 'emp-2',
		name: 'Eden Park',
		kind: 'agent',
		role: 'Principal Designer',
		availability: 'Paused'
	},
	{ id: 'emp-3', name: 'Alex Kim', kind: 'person', availability: 'Available' }
];

describe('smart task composer helpers', () => {
	it('detects @mention tokens at start or after whitespace and ignores email addresses', () => {
		expect(detectMentionMatch('@ka', 3)).toEqual({ start: 0, end: 3, query: 'ka' });
		expect(detectMentionMatch('Loop in @ed', 11)).toEqual({ start: 8, end: 11, query: 'ed' });
		expect(detectMentionMatch('mail@example.com', 16)).toBeNull();
		expect(detectMentionMatch('hello world', 11)).toBeNull();
	});

	it('filters employees by name, role, or kind', () => {
		expect(filterMentionCandidates(EMPLOYEES, 'design').map((e) => e.id)).toEqual(['emp-2']);
		expect(filterMentionCandidates(EMPLOYEES, 'kai').map((e) => e.id)).toEqual(['emp-1']);
		expect(filterMentionCandidates(EMPLOYEES, 'person').map((e) => e.id)).toEqual(['emp-3']);
	});

	it('replaces the active mention token with @FirstName and computes next caret', () => {
		const match = detectMentionMatch('Ask @ka to check', 7)!;
		expect(applyMentionReplacement('Ask @ka to check', match, EMPLOYEES[0])).toEqual({
			text: 'Ask @Kai  to check',
			caret: 9
		});
	});

	it('builds a Tess scoping prompt from draft title, description, and acceptance criteria', () => {
		expect(buildTessDraftPrompt('Audit EU office', 'Check disk space', ['Disk > 20%'])).toBe(
			'Help me scope this task: Audit EU office\n\nCheck disk space\n\nAcceptance criteria:\n- Disk > 20%'
		);
		expect(buildTessDraftPrompt('', '', [])).toContain('Help me scope a new task');
	});
});
