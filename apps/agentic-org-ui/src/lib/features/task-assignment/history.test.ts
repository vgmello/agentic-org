import { expect, it } from 'vitest';
import { eventSummary } from './history';
import type { ContentRef } from './wire';
const ref: ContentRef = {
	task_id: 'opaque',
	revision: '9223372036854775807',
	kind: 'event',
	sha256: 'a'.repeat(64),
	byte_length: 100
};
it('renders independent assignment fields with the full reference revision', () => {
	const source =
		'{"revision":9223372036854775807,"actor_kind":"owner","actor_id":null,"change":{"type":"assignment_changed","from":{"assigned_team_id":"team-a","assignee_id":"person"},"to":{"assigned_team_id":null,"assignee_id":"person"}},"extension":123456789012345678901234567890E+999}';
	expect(eventSummary(source, ref)).toBe(
		'Revision 9223372036854775807. Assignment changed from team team-a, assignee person to team None, assignee person. Actor: owner.'
	);
	expect(source).toContain('123456789012345678901234567890E+999');
});
it('makes unknown activity and unavailable baseline coverage explicit', () => {
	expect(eventSummary('{"change":{"type":"future_action"},"actor_kind":"unknown"}', ref)).toContain(
		'Unknown activity (future_action). Original event retained.'
	);
	expect(eventSummary('{"change":{"type":"history_baseline"}}', ref)).toContain(
		'Earlier history is unavailable.'
	);
	expect(eventSummary('[]', ref)).toContain('Unknown activity.');
});
