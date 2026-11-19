import { describe, expect, it } from 'vitest';
import { checkPersonName, handleOf, removePersonSummary, suggestName } from './people-edit';
import { sampleOrg } from './sample';

const s = sampleOrg('office-1');
const member = (id: string) => s.members.find((m) => m.id === id)!;

describe('PEOPLE-4 and PEOPLE-8 a person’s name is a handle', () => {
	it('is required and valid', () => {
		expect(checkPersonName(s, '  ', 'Acme')).toEqual({ ok: false, message: 'Enter a name.' });
		expect(checkPersonName(s, 'a\u0007b', 'Acme').ok).toBe(false);
		expect(checkPersonName(s, '  Sam Ortiz  ', 'Acme')).toEqual({ ok: true, name: 'Sam Ortiz' });
	});

	it('refuses another person’s name, ignoring case, says whose it is and suggests another', () => {
		expect(checkPersonName(s, 'avery stone', 'Acme')).toEqual({
			ok: false,
			message: 'Avery Stone is already in Acme. Use a different name, such as avery stone (2).'
		});
	});

	it('refuses an agent’s handle and the reserved owner', () => {
		expect(checkPersonName(s, 'SAM', 'Acme')).toEqual({ ok: false, message: "SAM is an agent's name. Choose another." });
		expect(checkPersonName(s, 'Owner', 'Acme')).toEqual({ ok: false, message: '"owner" is reserved for you. Choose another name.' });
	});

	it('an agent’s display name is not its handle', () => {
		expect(checkPersonName(s, 'Sam Okafor', 'Acme').ok).toBe(true);
	});

	it('a rename may keep the name in another case, but not take another’s', () => {
		expect(checkPersonName(s, 'AVERY STONE', 'Acme', 'avery').ok).toBe(true);
		expect(checkPersonName(s, 'Hana Weiss', 'Acme', 'avery').ok).toBe(false);
	});

	it('suggests the first free “Name (n)”', () => {
		const withTwo = { ...s, members: [...s.members, { id: 'x', kind: 'person' as const, name: 'Sam Ortiz (2)', version: 1 }] };
		expect(suggestName(withTwo, 'Sam Ortiz')).toBe('Sam Ortiz (3)');
		expect(suggestName(s, 'Sam Ortiz')).toBe('Sam Ortiz (2)');
	});

	it('an agent is addressed by its messaging name when it has one, else its id', () => {
		expect(handleOf(member('sam'))).toBe('sam');
		const profile = { ...member('sam'), agent: { role: 'r', title: 't', runtime: 'claude_code' as const, messaging_name: 'sam-bot', status: 'active' as const } };
		expect(handleOf(profile)).toBe('sam-bot');
		expect(handleOf(member('avery'))).toBe('Avery Stone');
	});
});

describe('PEOPLE-5 removing a person', () => {
	it('names the teams they leave and lead, who reports to whom instead, and what happens to messages and tasks', () => {
		const lines = removePersonSummary(s, member('hana'), 'Acme');
		expect(lines[0]).toBe('Hana Weiss leaves People, and stops leading People.');
		expect(lines[1]).toBe("Inês Duarte and Leo Brandt, who report to Hana Weiss, will report to Hana Weiss's manager, Avery Stone.");
		expect(lines.at(-1)).toContain("Messages waiting for Hana Weiss won't be delivered");
	});

	it('says "you" when they had no manager, and leaves out lines that do not apply', () => {
		const lines = removePersonSummary(s, member('avery'), 'Acme');
		expect(lines.some((l) => l.includes('leaves'))).toBe(false); // Avery is in no team
		expect(lines[0]).toBe('Hana Weiss, Juno Park and Sam Okafor, who report to Avery Stone, will report to you.');
	});
});
