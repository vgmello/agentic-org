import { describe, expect, it } from 'vitest';
import {
	checkSummary,
	compareVersions,
	holdBackNote,
	listSeparator,
	officeUpgradeHint,
	officeStanding,
	sortOffices,
	upgradeCommand,
	whenText
} from './present';
import type { OfficeVersion } from './types';

const office = (over: Partial<OfficeVersion>): OfficeVersion => ({
	id: 'o',
	name: 'Studio Mac',
	role: 'satellite',
	host: 'Studio Mac',
	instance: 'default',
	version: '0.4.0',
	holdsBack: false,
	...over
});

describe('compareVersions', () => {
	it('compares dotted parts as numbers', () => {
		expect(compareVersions('0.10.0', '0.9.2')).toBe(1);
		expect(compareVersions('0.9.2', '0.10.0')).toBe(-1);
		expect(compareVersions('1.2', '1.2.0')).toBe(0);
		expect(compareVersions('v1.2.3', '1.2.3')).toBe(0);
	});
	it('puts a pre-release before its release', () => {
		expect(compareVersions('1.0.0-rc.1', '1.0.0')).toBe(-1);
		expect(compareVersions('1.0.0', '1.0.0-rc.1')).toBe(1);
		expect(compareVersions('1.0.0-rc.2', '1.0.0-rc.10')).toBe(-1);
	});
	it('ignores build metadata', () => {
		expect(compareVersions('1.0.0+abc', '1.0.0+def')).toBe(0);
	});
	it("returns null for what isn't a version", () => {
		expect(compareVersions('nightly', '1.0.0')).toBeNull();
		expect(compareVersions('1.0.0', '')).toBeNull();
	});
});

describe('officeStanding', () => {
	it('says nothing for HQ', () => {
		expect(officeStanding(office({ role: 'hq' }), '0.4.0')).toBeNull();
	});
	it('compares with HQ in words', () => {
		expect(officeStanding(office({ version: '0.4.0' }), '0.4.0')?.word).toBe('Same as HQ');
		expect(officeStanding(office({ version: '0.3.1' }), '0.4.0')?.word).toBe('Older than HQ');
		expect(officeStanding(office({ version: '0.5.0' }), '0.4.0')?.word).toBe('Newer than HQ');
		expect(officeStanding(office({ version: null }), '0.4.0')?.word).toBe('Version unknown');
		expect(officeStanding(office({ version: 'dev' }), '0.4.0')?.word).toBe('Version unknown');
	});
	it('is amber only when the office holds features back', () => {
		expect(officeStanding(office({ version: '0.3.1', holdsBack: true }), '0.4.0')).toEqual({
			word: 'Needs update',
			tone: 'owner'
		});
		expect(officeStanding(office({ version: '0.3.1' }), '0.4.0')?.tone).toBe('neutral');
	});
});

describe('sortOffices', () => {
	it('puts HQ first, then A–Z', () => {
		const sorted = sortOffices([
			office({ name: 'studio' }),
			office({ name: 'Build box' }),
			office({ name: 'Main', role: 'hq' })
		]);
		expect(sorted.map((o) => o.name)).toEqual(['Main', 'Build box', 'studio']);
	});
});

describe('holdBackNote', () => {
	it('uses the offices spec sentence for one office, and names it for its link', () => {
		const note = holdBackNote([
			office({ id: 'b', name: 'Build box', holdsBack: true }),
			office({})
		]);
		expect(note?.text).toBe(
			'Build box runs an older version of Agentic Org, so new features wait until it updates.'
		);
		expect(note?.held.map((o) => o.id)).toEqual(['b']);
		expect(note?.tail).toBe(
			' runs an older version of Agentic Org, so new features wait until it updates.'
		);
	});
	it('names several offices', () => {
		expect(
			holdBackNote([
				office({ id: 'a', name: 'A', holdsBack: true }),
				office({ id: 'b', name: 'B', holdsBack: true }),
				office({ id: 'c', name: 'C', holdsBack: true })
			])?.text
		).toBe(
			'A, B and C run an older version of Agentic Org, so new features wait until they update.'
		);
	});
	it('is absent when nothing is held back', () => {
		expect(holdBackNote([office({})])).toBeNull();
	});
	it('separates linked names as the sentence does', () => {
		expect([0, 1, 2].map((i) => listSeparator(i, 3))).toEqual(['', ', ', ' and ']);
		expect([0, 1].map((i) => listSeparator(i, 2))).toEqual(['', ' and ']);
	});
});

describe('officeUpgradeHint', () => {
	it("names the office's own instance, never the machine's default by omission", () => {
		expect(officeUpgradeHint(office({ instance: 'build' }))).toEqual({
			command: 'agentic-org upgrade --name build',
			known: true
		});
	});
	it("asks for the instance's name when HQ doesn't know it", () => {
		const hint = officeUpgradeHint(office({ instance: null }));
		expect(hint).toEqual({ command: 'agentic-org upgrade --name <instance>', known: false });
		expect(hint.command).toContain('--name');
	});
});

describe('upgradeCommand', () => {
	it('names the instance', () => {
		expect(upgradeCommand('default')).toBe('agentic-org upgrade --name default');
		expect(upgradeCommand('lab-2')).toBe('agentic-org upgrade --name lab-2');
	});
	it('quotes a name the shell would split', () => {
		expect(upgradeCommand("my lab's")).toBe(`agentic-org upgrade --name 'my lab'\\''s'`);
	});
});

describe('whenText', () => {
	const now = Date.parse('2026-11-19T12:00:00Z');
	it('is relative within 24 hours', () => {
		expect(whenText('2026-11-19T11:59:40Z', now)).toBe('just now');
		expect(whenText('2026-11-19T11:59:00Z', now)).toBe('1 minute ago');
		expect(whenText('2026-11-19T11:15:00Z', now)).toBe('45 minutes ago');
		expect(whenText('2026-11-19T10:00:00Z', now)).toBe('2 hours ago');
	});
	it('is an absolute 24-hour time after that', () => {
		expect(whenText('2026-11-17T14:02:00Z', now, 'UTC')).toBe('on 17 Nov 2026, 14:02');
	});
	it('does not invent a time', () => {
		expect(whenText('not a date', now)).toBe('at an unknown time');
	});
});

describe('checkSummary', () => {
	const now = Date.parse('2026-11-19T12:00:00Z');
	const at = '2026-11-19T10:00:00Z';
	it('words every state', () => {
		expect(checkSummary({ state: 'up_to_date', checkedAt: at }, now).word).toBe('Up to date');
		const available = checkSummary(
			{ state: 'available', version: '0.5.0', checkedAt: at, staged: true },
			now
		);
		expect(available.word).toBe('Update available');
		expect(available.detail).toContain('Version 0.5.0 is available, downloaded and verified');
		expect(available.detail).toContain('Nothing changes until you upgrade.');
		expect(
			checkSummary({ state: 'available', version: '0.5.0', checkedAt: at, staged: false }, now)
				.detail
		).not.toContain('downloaded');
		const failed = checkSummary(
			{ state: 'failed', checkedAt: at, reason: "HQ couldn't reach the release server" },
			now
		);
		expect(failed).toMatchObject({ word: 'Check failed', tone: 'bad' });
		expect(failed.detail).toBe(
			"The last check for a newer release failed 2 hours ago: HQ couldn't reach the release server. This instance keeps running its current release."
		);
		expect(checkSummary({ state: 'offline' }, now).word).toBe('Offline');
		expect(checkSummary({ state: 'unknown' }, now).word).toBe('Not reported');
	});
});
