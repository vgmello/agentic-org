import { describe, expect, it } from 'vitest';
import { priorityTone, severityTone, statusTone } from './tones';

describe('work tones', () => {
	it('maps task statuses to unified semantic tones', () => {
		expect(statusTone('task', 'done')).toBe('ok');
		expect(statusTone('task', 'blocked')).toBe('bad');
		expect(statusTone('task', 'in_progress')).toBe('accent');
		expect(statusTone('task', 'todo')).toBe('neutral');
	});

	it('maps bug statuses to unified semantic tones', () => {
		expect(statusTone('bug', 'fixed')).toBe('ok');
		expect(statusTone('bug', 'closed')).toBe('ok');
		expect(statusTone('bug', 'new')).toBe('warn');
		expect(statusTone('bug', 'triaged')).toBe('accent');
		expect(statusTone('bug', 'in_progress')).toBe('accent');
	});

	it('maps request statuses to unified semantic tones', () => {
		expect(statusTone('request', 'approved')).toBe('ok');
		expect(statusTone('request', 'scoped')).toBe('warn');
		expect(statusTone('request', 'routed')).toBe('accent');
		expect(statusTone('request', 'scoping')).toBe('accent');
		expect(statusTone('request', 'declined')).toBe('neutral');
	});

	it('maps priority and severity to semantic tones', () => {
		expect(priorityTone('urgent')).toBe('bad');
		expect(priorityTone('high')).toBe('warn');
		expect(priorityTone('normal')).toBe('neutral');
		expect(priorityTone('low')).toBe('neutral');
		expect(severityTone('critical')).toBe('bad');
		expect(severityTone('major')).toBe('warn');
		expect(severityTone('minor')).toBe('neutral');
		expect(severityTone('trivial')).toBe('neutral');
		expect(severityTone(undefined)).toBe('neutral');
	});
});
