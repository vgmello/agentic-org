// What the people dialogs need, as pure functions over a snapshot (PEOPLE-3, 4, 5, 8): a person's name is a
// handle that agents address them by, unique across every member ignoring case, so the clash is answered
// before anything is sent (HQ repeats the check), and what removing someone does is said in words.
import { trimWhiteSpace, validateName } from '$lib/name';
import type { Member, OrgSnapshot } from '$lib/proto';
import { byName, kindWord, membersOfTeam, teamsOfMember } from './model';
import { managerOf } from './reporting';

const OWNER_HANDLE = 'owner';
const same = (a: string, b: string) => trimWhiteSpace(a).toLowerCase() === trimWhiteSpace(b).toLowerCase();

/**
 * The handle a member is addressed by: a person's name, an agent's messaging name. The mock's agents carry
 * one only when the Rust mock office sent a profile; otherwise the id stands in (as in the mock offices).
 */
export const handleOf = (m: Member): string => (m.kind === 'agent' ? (m.agent?.messaging_name ?? m.id) : m.name);

export type PersonNameCheck = { ok: true; name: string } | { ok: false; message: string };

/** A name that isn't taken: "Sam Ortiz (2)", "(3)"…, the first whose handle is free. */
export function suggestName(s: OrgSnapshot, name: string, exceptId?: string): string {
	for (let n = 2; ; n++) {
		const candidate = `${name} (${n})`;
		if (!handleTaken(s, candidate, exceptId)) return candidate;
	}
}

/** Whose handle `name` is, if anyone's: another member, or the owner's reserved `owner`. */
function handleHolder(s: OrgSnapshot, name: string, exceptId?: string): Member | 'owner' | undefined {
	if (same(name, OWNER_HANDLE)) return 'owner';
	return s.members.find((m) => m.id !== exceptId && !m.owner && same(handleOf(m), name));
}
const handleTaken = (s: OrgSnapshot, name: string, exceptId?: string) => handleHolder(s, name, exceptId) !== undefined;

/**
 * A person's name before sending (PEOPLE-4, PEOPLE-8): required and valid, then free as a handle. A clash
 * says whose it is and suggests another. `exceptId` is the person being renamed.
 */
export function checkPersonName(s: OrgSnapshot, raw: string, organization: string, exceptId?: string): PersonNameCheck {
	const checked = validateName(raw, 'Enter a name.');
	if (!checked.ok) return checked;
	const holder = handleHolder(s, checked.name, exceptId);
	if (holder === 'owner') return { ok: false, message: '"owner" is reserved for you. Choose another name.' };
	if (holder?.kind === 'agent') return { ok: false, message: `${checked.name} is an agent's name. Choose another.` };
	if (holder) return { ok: false, message: `${holder.name} is already in ${organization}. Use a different name, such as ${suggestName(s, checked.name, exceptId)}.` };
	return checked;
}

/** The lines of the removal confirmation (PEOPLE-5): only what applies. HQ refuses the owner. */
export function removePersonSummary(s: OrgSnapshot, m: Member, organization: string): string[] {
	const lines: string[] = [];
	const teams = teamsOfMember(s, m.id);
	const led = teams.filter((t) => t.lead_member_id === m.id);
	if (teams.length > 0) {
		const names = list(teams.map((t) => t.name));
		lines.push(led.length > 0 ? `${m.name} leaves ${names}, and stops leading ${list(led.map((t) => t.name))}.` : `${m.name} leaves ${names}.`);
	}
	const reports = s.members.filter((x) => !x.owner && x.reports_to_member_id === m.id).sort(byName);
	if (reports.length > 0) {
		const boss = managerOf(s, m);
		const to = boss && !boss.owner ? boss.name : 'you';
		lines.push(`${list(reports.map((r) => r.name))}, who ${reports.length === 1 ? 'reports' : 'report'} to ${m.name}, will report to ${boss && !boss.owner ? `${m.name}'s manager, ${to}` : to}.`);
	}
	lines.push(`Agents can no longer reach ${m.name}. Messages waiting for ${m.name} won't be delivered, and ${m.name}'s open tasks come back unassigned for you to reassign.`);
	void organization;
	return lines;
}

/** "A", "A and B", "A, B and C". */
const list = (names: string[]) => (names.length <= 1 ? (names[0] ?? '') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`);

/** What a member's quick view says about the people around them: who they report to and who reports to them. */
export function relationsOf(s: OrgSnapshot, m: Member): { manager: Member | undefined; reports: Member[]; peopleInTeams: number } {
	const manager = managerOf(s, m);
	const reports = s.members.filter((x) => !x.owner && x.reports_to_member_id === m.id).sort(byName);
	const peopleInTeams = teamsOfMember(s, m.id).reduce((n, t) => n + membersOfTeam(s, t.id).filter((x) => x.kind === 'person').length, 0);
	void kindWord;
	return { manager, reports, peopleInTeams };
}
