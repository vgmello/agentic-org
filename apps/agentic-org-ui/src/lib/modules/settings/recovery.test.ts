import { describe, expect, it } from 'vitest';
import {
	messageText,
	recoveryMessage,
	releaseCommand,
	NOT_SUPPORTED,
	type RecoveryContext
} from './recovery';
import { shellQuote } from '$lib/platform/organizations/command';

const ctx: RecoveryContext = {
	organization: 'Lumen Labs',
	hqMachine: 'Studio Mac',
	instance: 'default',
	viewerIsAdministrator: true,
	date: (v) => v.slice(0, 10)
};

describe('shellQuote', () => {
	it('quotes with single quotes and writes an embedded quote as the escape', () => {
		expect(shellQuote("Ada's $lab")).toBe(`'Ada'\\''s $lab'`);
	});
});

describe('recoveryMessage', () => {
	it('says being prepared', () => {
		expect(messageText(recoveryMessage({ state: 'preparing' }, ctx))).toBe(
			'Recovery code: being prepared. This takes a moment.'
		);
	});
	it('gives the quoted command while waiting, and says who runs it for a non-administrator', () => {
		const m = recoveryMessage({ state: 'waiting' }, ctx);
		expect(m.command).toBe("agentic-org recovery release --org 'Lumen Labs'");
		expect(messageText(m)).toBe(
			"Recovery code: waiting to be collected on the HQ machine. Run `agentic-org recovery release --org 'Lumen Labs'` on Studio Mac."
		);
		expect(
			messageText(recoveryMessage({ state: 'waiting' }, { ...ctx, viewerIsAdministrator: false }))
		).toContain("This HQ's administrator runs it.");
	});
	it('adds --name when the instance is not default', () => {
		expect(releaseCommand('Lumen Labs', 'lab')).toBe(
			"agentic-org recovery release --org 'Lumen Labs' --name 'lab'"
		);
	});
	it('counts second showings', () => {
		const once = messageText(
			recoveryMessage(
				{ state: 'shown', shownOn: '2026-11-10', again: { count: 1, lastOn: '2026-11-12' } },
				ctx
			)
		);
		expect(once).toContain('(shown again once, on 2026-11-12), but not confirmed as stored');
		const twice = messageText(
			recoveryMessage(
				{ state: 'shown', shownOn: '2026-11-10', again: { count: 2, lastOn: '2026-11-12' } },
				ctx
			)
		);
		expect(twice).toContain(
			'shown on Studio Mac on 2026-11-10 (shown again 2 times, last on 2026-11-12), but not confirmed as stored.'
		);
		expect(twice).toContain('--redeliver');
	});
	it('leaves out a date HQ did not give', () => {
		expect(messageText(recoveryMessage({ state: 'collected' }, ctx))).toBe(
			'Recovery code collected.'
		);
		expect(messageText(recoveryMessage({ state: 'shown' }, ctx))).toContain(
			'shown on Studio Mac, but not'
		);
	});
	it('reads collected after second showings', () => {
		expect(
			messageText(
				recoveryMessage(
					{
						state: 'collected',
						shownOn: '2026-11-10',
						collectedOn: '2026-11-13',
						again: { count: 2, lastOn: '2026-11-12' }
					},
					ctx
				)
			)
		).toBe(
			'Recovery code shown on 2026-11-10 (shown again 2 times, last on 2026-11-12) · collected on 2026-11-13.'
		);
	});
	it('gives the reason and the doctor command when it could not be prepared', () => {
		const m = recoveryMessage({ state: 'failed', reason: 'The package could not be written' }, ctx);
		expect(messageText(m)).toBe(
			"Recovery code: couldn't be prepared. The package could not be written. On Studio Mac, run `agentic-org doctor`."
		);
	});
	it('never calls an unknown state collected', () => {
		expect(messageText(recoveryMessage({ state: 'unknown' }, ctx))).toBe(
			'Recovery code: status unknown.'
		);
	});
	it('exports the not-supported sentence', () => {
		expect(NOT_SUPPORTED).toBe(
			"Recovering an organization after losing the HQ machine isn't supported yet."
		);
	});
});
