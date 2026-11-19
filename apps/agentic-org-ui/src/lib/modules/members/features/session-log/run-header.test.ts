import { describe, expect, it } from 'vitest';
import { runFacts } from './run-header';

describe('session log header', () => {
	it('names the run: when, who started it, how long and the outcome', () => {
		const facts = runFacts({
			id: 't',
			startedAt: '2026-11-18T09:00:00Z',
			endedAt: '2026-11-18T09:04:30Z',
			startedBy: { kind: 'message', by: { id: 'm', name: 'Mira Castillo' } },
			state: 'succeeded',
			summary: 'Fixed the deploy script',
			attempts: [],
			hasLog: true
		});
		expect(facts.slice(1)).toEqual([
			'Message from Mira Castillo',
			'ran 4 min',
			'Fixed the deploy script'
		]);
	});
});
