import { describe, expect, it } from 'vitest';
import { agentLine, filterRoster, isPaused, messagingNameError, offboardSummary, roster, runLimitsText, runModeLabel, runtimeLabel, scheduleLabel, agentStatus, suggestMessagingName } from './agents';
import { formerMembers, isFormer, withoutFormer } from './model';
import { sampleOrg } from './sample';

const s = sampleOrg('office-1');
const member = (id: string) => s.members.find((m) => m.id === id)!;
const left = (id: string, at = '2026-11-08T10:00:00.000Z') => ({ ...s, members: s.members.map((m) => (m.id === id ? { ...m, agent: { ...m.agent!, status: 'offboarded' as const, offboarded_at: at } } : m)) });

describe('the words of an agent’s profile', () => {
	it('names the runtime, a named other as given, and says plainly when it is not set', () => {
		expect(['claude_code', 'codex', 'antigravity'].map((r) => runtimeLabel(r as never))).toEqual(['Claude Code', 'Codex', 'Antigravity']);
		expect(runtimeLabel({ other: 'my-runtime' })).toBe('my-runtime');
		expect(runtimeLabel('unknown')).toBe('Runtime not set');
		expect(runtimeLabel(undefined)).toBe('Runtime not set');
		expect(runtimeLabel({ other: '' })).toBe('Runtime not set');
	});

	it('says how it runs; no mode means a person starts it', () => {
		expect(runModeLabel('on_call')).toBe('On call');
		expect(runModeLabel('always_on')).toBe('Always on');
		expect(runModeLabel('scheduled')).toBe('Scheduled');
		expect(runModeLabel(undefined)).toBe('Started by a person');
		expect(runModeLabel('unknown')).toBe('Run mode not recognised');
	});

	it('words the schedule with its time zone', () => {
		expect(scheduleLabel({ spec: 'weekdays at 9', time_zone: 'Europe/Lisbon' })).toBe('weekdays at 9 (Europe/Lisbon)');
		expect(scheduleLabel(undefined)).toBe('');
	});

	it('the status word never reads unknown as working', () => {
		const sam = member('sam').agent!;
		// "Active" is the admin flag, never shown as a status: an on-call agent with no live run is asleep.
		expect(agentStatus({ ...sam, run_mode: 'on_call' })).toEqual({ id: 'asleep', word: 'Asleep', detail: 'wakes when called' });
		expect(agentStatus({ ...sam, run_mode: undefined })).toMatchObject({ id: 'by_person', word: 'Started by a person' });
		expect(agentStatus({ ...sam, run_mode: 'always_on' })).toMatchObject({ id: 'unknown', word: 'Status unknown' });
		expect(agentStatus({ ...sam, status: 'paused' })).toMatchObject({ id: 'paused', word: 'Paused' });
		expect(agentStatus({ ...sam, status: 'offboarded' })).toMatchObject({ word: 'Former' });
		expect(agentStatus({ ...sam, status: 'unknown' }).word).toBe('Status unknown');
		expect(agentStatus(undefined).word).toBe('Status unknown');
		for (const mode of [undefined, 'on_call', 'always_on', 'scheduled'] as const)
			expect(agentStatus({ ...sam, run_mode: mode }).word).not.toBe('Active');
		expect(isPaused({ ...member('sam'), agent: { ...member('sam').agent!, status: 'paused' } })).toBe(true);
	});

	it('one line for a quick view', () => {
		expect(agentLine(member('sam'))).toBe('software engineer · Runs on Claude Code · On call');
		expect(agentLine(member('avery'))).toBe('');
	});
});

describe('former members', () => {
	it('an offboarded agent is left out of the chart and listed apart, most recent first', () => {
		const both = { ...left('sam', '2026-11-08T10:00:00.000Z'), members: left('sam').members.map((m) => (m.id === 'kai' ? { ...m, agent: { ...m.agent!, status: 'offboarded' as const, offboarded_at: '2026-11-08T11:00:00.000Z' } } : m)) };
		expect(isFormer(both.members.find((m) => m.id === 'sam')!)).toBe(true);
		expect(withoutFormer(both).members.map((m) => m.id)).not.toContain('sam');
		expect(withoutFormer(both).members.map((m) => m.id)).toContain('noor');
		expect(formerMembers(both).map((m) => m.id)).toEqual(['kai', 'sam']);
	});

	it('nothing changes when nobody has left', () => {
		expect(withoutFormer(s)).toBe(s);
		expect(formerMembers(s)).toEqual([]);
	});
});

describe('DEAL-4 what offboarding says', () => {
	it('names the teams left and led, who reports to whom, the open tasks, and the reserved name', () => {
		const lines = offboardSummary(s, member('sam'));
		expect(lines[0]).toBe('Sam Okafor leaves Engineering, and stops leading Engineering.');
		expect(lines[1]).toBe('Any open tasks Sam Okafor has come back unassigned for you to reassign.');
		expect(lines[2]).toBe("Kai Lindqvist, Mira Castillo and Noor Haddad, who report to Sam Okafor, will report to Sam Okafor's manager, Avery Stone.");
		expect(lines.at(-1)).toBe("Sam Okafor's record stays, and the name sam stays reserved: nobody else can use it.");
	});

	it('leaves out what does not apply', () => {
		const lines = offboardSummary(s, member('quinn'));
		expect(lines.some((l) => l.includes('leaves'))).toBe(false);
		expect(lines.some((l) => l.includes('report'))).toBe(false);
	});
});

describe('messaging names', () => {
	it('must have the contract’s shape', () => {
		expect(messagingNameError(s, '')).toContain('Use 1 to 64');
		expect(messagingNameError(s, '-cam')).toContain('Use 1 to 64');
		expect(messagingNameError(s, 'has space')).toContain('Use 1 to 64');
		expect(messagingNameError(s, 'ac-qa-cameron')).toBeUndefined();
	});

	it('are handles: taken by an agent, a person or the owner, ignoring case', () => {
		expect(messagingNameError(s, 'SAM')).toBe("SAM is already Sam Okafor's messaging name. Choose another.");
		expect(messagingNameError(s, 'Hana Weiss'.replace(' ', '-'))).toBeUndefined(); // a person's display name is a different string
		expect(messagingNameError(s, 'Owner')).toBe('"owner" is reserved for the owner. Choose another.');
		expect(messagingNameError(s, 'sam', 'sam')).toBeUndefined(); // an agent may keep its own
	});

	it('suggests one from the name', () => {
		expect(suggestMessagingName('Cameron Díaz')).toBe('cameron-diaz');
		expect(suggestMessagingName('  ')).toBe('');
	});
});

describe('the Agents roster', () => {
	const offices = [{ id: 'office-1', name: 'HQ' }];
	it('lists every agent A–Z with its teams, office, manager and status', () => {
		const rows = roster(s, offices);
		expect(rows.length).toBe(8);
		expect(rows.map((r) => r.member.name)).toEqual([...rows.map((r) => r.member.name)].sort((a, b) => a.localeCompare(b)));
		const noor = rows.find((r) => r.member.id === 'noor')!;
		expect(noor).toMatchObject({ teams: ['Design', 'Web'], office: 'HQ', manager: 'Sam Okafor', status: 'active' });
		expect(rows.find((r) => r.member.id === 'ines')!.office).toBe('Unplaced');
		expect(rows.find((r) => r.member.id === 'quinn')!.manager).toBe('Alex Kim'); // no manager: the owner
	});

	it('filters by status; All leaves out agents who left', () => {
		const withStates = {
			...s,
			members: s.members.map((m) => (m.id === 'sam' ? { ...m, agent: { ...m.agent!, status: 'paused' as const } } : m.id === 'kai' ? { ...m, agent: { ...m.agent!, status: 'offboarded' as const } } : m))
		};
		const rows = roster(withStates, offices);
		expect(filterRoster(rows, 'all').length).toBe(7);
		expect(filterRoster(rows, 'paused').map((r) => r.member.id)).toEqual(['sam']);
		expect(filterRoster(rows, 'left').map((r) => r.member.id)).toEqual(['kai']);
		expect(filterRoster(rows, 'active').length).toBe(6);
	});
});

describe('RUN-6 the limits that end a run', () => {
	it('says when an on call or scheduled agent sleeps and when a run is stopped', () => {
		for (const mode of ['on_call', 'scheduled']) {
			expect(runLimitsText(mode)).toBe('Defaults for all agents: it goes back to sleep after 5 minutes with nothing to do; a run stops after 30 minutes of work in its home, or 2 hours on a task.');
		}
	});
	it('gives an always on agent only the busy limits, and a hand-started or unknown mode none', () => {
		expect(runLimitsText('always_on')).toBe('Defaults for all agents: a run stops after 30 minutes of work in its home, or 2 hours on a task.');
		expect(runLimitsText('')).toBe('');
		expect(runLimitsText('unknown')).toBe('');
	});
});
