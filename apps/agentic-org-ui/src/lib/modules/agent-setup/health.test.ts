import { describe, expect, it } from 'vitest';
import {
	health,
	healthText,
	modelMissing,
	needsSetupCount,
	setupReasons,
	type HealthFacts
} from './health';

const ready: HealthFacts = {
	name: 'Noor',
	status: 'active',
	homeTeam: { id: 'web', name: 'Web' },
	execution: {
		profile: { id: 'primary-pool', name: 'Primary pool' },
		modelId: 'claude-sonnet-5',
		enabled: true,
		runtimes: ['Claude Code']
	},
	placement: {
		office: { id: 'hq', name: 'HQ' },
		generation: 2,
		observedAt: '2026-11-19T10:00:00Z'
	},
	readiness: { known: true, checkedAt: '2026-11-19T10:00:00Z', blockers: [] },
	catalog: {
		reported: true,
		models: ['claude-sonnet-5', 'claude-opus-5'],
		asOf: '2026-11-19T10:00:00Z'
	}
};

describe('health', () => {
	it('is Ready only from known readiness with nothing missing', () => {
		expect(health(ready)).toEqual({ kind: 'ready' });
		expect(healthText(health(ready))).toBe('Ready');
	});

	it('says readiness is unknown rather than ready when the office can not tell', () => {
		const h = health({ ...ready, readiness: { known: false, why: "The office hasn't reported." } });
		expect(h).toEqual({ kind: 'unknown', why: "The office hasn't reported." });
		expect(healthText(h)).toBe('Runtime readiness unknown');
	});

	it('is Paused for a paused agent with nothing to set up, even when readiness is unknown', () => {
		expect(health({ ...ready, status: 'paused' }).kind).toBe('paused');
		expect(health({ ...ready, status: 'paused', readiness: { known: false, why: 'x' } }).kind).toBe(
			'paused'
		);
	});

	it('puts Needs setup before Paused', () => {
		const h = health({ ...ready, status: 'paused', execution: null });
		expect(h.kind).toBe('needs_setup');
		expect(healthText(h)).toBe('Needs setup: Execution not configured');
	});

	it('names every reason in fixing order and counts the rest in the short form', () => {
		const h = health({
			...ready,
			homeTeam: undefined,
			placement: null,
			readiness: {
				known: true,
				checkedAt: 'x',
				blockers: [
					{ code: 'AUTH_EXPIRED', reason: "Codex isn't signed in on HQ", fix: 'Run codex login.' }
				]
			}
		});
		expect(h.kind === 'needs_setup' && h.reasons.map((r) => r.code)).toEqual([
			'HOME_TEAM_MISSING',
			'UNPLACED',
			'AUTH_EXPIRED'
		]);
		expect(healthText(h)).toBe(
			'Needs setup: Home team missing: this record is invalid (and 2 more)'
		);
	});

	it('treats a disabled execution configuration as needing setup, not as a second pause', () => {
		const h = health({ ...ready, execution: { ...ready.execution!, enabled: false } });
		expect(healthText(h)).toBe('Needs setup: Execution is turned off in its configuration');
		// Document 11 defines no write that turns it on with a profile or model save: nothing here promises one.
		const reason = h.kind === 'needs_setup' ? h.reasons[0] : undefined;
		expect(reason?.fix).toBe(
			"The office admits no turn while it's off. Turning it on isn't available here yet."
		);
		expect(reason?.fix).not.toMatch(/Edit|sav/i);
	});
});

it('needs setup when the profile has no connection, even if the office reports no blocker', () => {
	const h = health({ ...ready, execution: { ...ready.execution!, runtimes: [] } });
	expect(healthText(h)).toBe('Needs setup: Primary pool has no connection to run on');
});

describe('the strict missing-model predicate', () => {
	it('holds only for an exact selected model known absent from a reported list', () => {
		const missing = {
			...ready,
			catalog: { reported: true as const, models: ['claude-opus-5'], asOf: 'x' }
		};
		expect(modelMissing(missing)).toBe(true);
		expect(setupReasons(missing)).toEqual([
			expect.objectContaining({
				code: 'MODEL_NOT_ENTITLED',
				text: "claude-sonnet-5 isn't offered by Claude Code on HQ"
			})
		]);
	});

	it('does not hold for an unset model, an unreadable list, no runtime or no office', () => {
		const absent = { reported: true as const, models: [], asOf: 'x' };
		expect(modelMissing({ ...ready, execution: null, catalog: absent })).toBe(false);
		expect(modelMissing({ ...ready, catalog: { reported: false } })).toBe(false);
		expect(
			modelMissing({ ...ready, catalog: absent, execution: { ...ready.execution!, runtimes: [] } })
		).toBe(false);
		expect(modelMissing({ ...ready, catalog: absent, placement: null })).toBe(false);
		expect(
			modelMissing({ ...ready, catalog: absent, execution: { ...ready.execution!, modelId: '' } })
		).toBe(false);
	});

	it('is not said twice when the office already reports the model as not offered', () => {
		const facts: HealthFacts = {
			...ready,
			catalog: { reported: true, models: [], asOf: 'x' },
			readiness: {
				known: true,
				checkedAt: 'x',
				blockers: [{ code: 'MODEL_NOT_ENTITLED', reason: 'Not entitled to claude-sonnet-5' }]
			}
		};
		expect(setupReasons(facts)).toHaveLength(1);
	});
});

it('counts the agents that need setup', () => {
	expect(
		needsSetupCount([ready, { ...ready, placement: null }, { ...ready, status: 'paused' }])
	).toBe(1);
});
