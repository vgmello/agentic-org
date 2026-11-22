// What the person is told after Run now (app-shell-ux §6a: a success toast needs a confirmed outcome). HQ accepting
// the request is not a started turn: until the office reports the turn running, the words say "requested".
import { outcomeText } from '../../present';
import type { Turn } from '../../types';

export interface RunNowNotice {
	/** `success` only for a turn the office confirmed running or finished; `info` otherwise. */
	tone: 'success' | 'info';
	title: string;
	description?: string;
}

export function runNowNotice(turn: Turn, name: string): RunNowNotice {
	switch (turn.state) {
		case 'running':
		case 'switching':
			return { tone: 'success', title: `Started a turn for ${name}.` };
		case 'succeeded':
			return { tone: 'success', title: `${name} finished the turn.` };
		case 'accepted':
			return {
				tone: 'info',
				title: `Run requested for ${name}.`,
				description: 'It starts when the office confirms it; this page updates when it does.'
			};
		default:
			return { tone: 'info', title: `${name}'s run didn't start.`, description: outcomeText(turn) };
	}
}
