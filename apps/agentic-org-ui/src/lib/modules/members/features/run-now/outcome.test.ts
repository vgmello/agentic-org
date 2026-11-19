import { describe, expect, it } from 'vitest';
import type { Turn } from '../../types';
import { runNowNotice } from './outcome';

const turn = (state: Turn['state']): Turn => ({
	id: 't',
	startedAt: '2026-11-18T09:00:00Z',
	startedBy: { kind: 'run', byViewer: true },
	state,
	attempts: [],
	hasLog: false
});

describe('Run now notice', () => {
	it('says "requested", not started, until the office confirms', () => {
		expect(runNowNotice(turn('accepted'), 'Kai')).toMatchObject({
			tone: 'info',
			title: 'Run requested for Kai.'
		});
	});
	it('claims a started turn only when it is running', () => {
		expect(runNowNotice(turn('running'), 'Kai')).toEqual({
			tone: 'success',
			title: 'Started a turn for Kai.'
		});
	});
	it('never shows a failed start as success', () => {
		expect(runNowNotice({ ...turn('failed'), failure: 'no seat' }, 'Kai')).toMatchObject({
			tone: 'info',
			description: 'Failed: no seat'
		});
	});
});
