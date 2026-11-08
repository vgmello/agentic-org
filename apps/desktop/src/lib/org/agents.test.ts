import { describe, expect, it } from 'vitest';
import { agentLine, isPaused, messagingNameError, offboardSummary, runModeLabel, runtimeLabel, scheduleLabel, statusWord, suggestMessagingName } from './agents';
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
		expect(statusWord(member('sam').agent)).toBe('Active');
		expect(statusWord({ ...member('sam').agent!, status: 'paused' })).toBe('Paused');
		expect(statusWord({ ...member('sam').agent!, status: 'unknown' })).toBe('Status unknown');
		expect(statusWord(undefined)).toBe('Status unknown');
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
