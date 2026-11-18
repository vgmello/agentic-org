import { describe, expect, it } from 'vitest';
import { nameList, removeOfficeText, waitingText } from './copy';

describe('removeOfficeText', () => {
	it('names the office, its agents, their tasks and the offline case', () => {
		expect(removeOfficeText({ office: 'Studio Mac', organization: 'Lumen Labs', agents: 2, tasks: 5, offline: true })).toBe(
			"Remove Studio Mac from Lumen Labs? It can't sync again. Its 2 agents stop running there and become unplaced: you choose a new office for each, or offboard them. HQ takes back their 5 tasks. If Studio Mac is offline, it keeps its copy until it next connects, then deletes it. To add it again, send a new invitation."
		);
	});
	it('uses singular forms and leaves out what does not apply', () => {
		const text = removeOfficeText({ office: 'Build box', organization: 'Lumen Labs', agents: 1, tasks: 1, offline: false });
		expect(text).toContain('Its agent stops running there and becomes unplaced: you choose a new office for it, or offboard it. HQ takes back its 1 task.');
		expect(text).not.toContain('offline');
		expect(removeOfficeText({ office: 'X', organization: 'Y', agents: 0, tasks: 0, offline: false })).toBe("Remove X from Y? It can't sync again. To add it again, send a new invitation.");
	});
});

describe('waitingText', () => {
	it('counts and names the agents', () => {
		expect(waitingText(['Kai', 'Juno', 'Nadia'])).toBe('Waiting for 3 agents to stop: Kai, Juno and Nadia. Their active work and process cleanup must finish first');
		expect(nameList(['Kai'])).toBe('Kai');
	});
});
