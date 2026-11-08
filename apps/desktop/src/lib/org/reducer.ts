// Applies one organization command to a snapshot with the contract's rules (crates/org-proto/src/org.rs):
// what HQ validates. The TEMPORARY browser mock uses it so the UI can be built and tested without the app;
// the Rust shell's mock HQ (src-tauri/src/org_engine.rs) follows the same rules, and test-vectors/org-commands.json
// is run against both. The real HQ is the authority. Pure: it returns a new snapshot, or throws the same
// error codes HQ would, in the contract's order: not_found, invalid_request, invalid_name, then the command's own rule
// (owner_protected, cycle, not_a_member, not_empty), conflict, then already_exists: a permanent refusal is never hidden
// behind a stale version.
import type { AgentProfile, AgentRuntime, ApiErrorCode, AppError, Area, Member, Membership, OrgSnapshot, ProjectSummary, ProjectTeam, RunMode, Schedule, Team } from '$lib/proto';
import { trimWhiteSpace, validateName } from '$lib/name';
import { withoutNulls, type OrgCommand } from './commands';

const fail = (code: ApiErrorCode, message: string): never => {
	throw { source: 'api', code, message } satisfies AppError;
};

const sameName = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

function checkedName(raw: string): string {
	const checked = validateName(raw);
	return checked.ok ? checked.name : fail('invalid_name', checked.message);
}

const owner = (s: OrgSnapshot) => s.members.find((m) => m.owner);

/** Every team below `teamId`, directly or through others. */
export function teamsBelow(s: OrgSnapshot, teamId: string): Set<string> {
	const below = new Set<string>();
	const walk = (id: string) => s.teams.filter((t) => t.owner_team_id === id).forEach((t) => !below.has(t.id) && (below.add(t.id), walk(t.id)));
	walk(teamId);
	return below;
}

/** Everyone who reports to `memberId`, directly or through others. */
export function reportsBelow(s: OrgSnapshot, memberId: string): Set<string> {
	const below = new Set<string>();
	const walk = (id: string) =>
		s.members.filter((m) => !m.owner && m.reports_to_member_id === id).forEach((m) => !below.has(m.id) && m.id !== memberId && (below.add(m.id), walk(m.id)));
	walk(memberId);
	return below;
}

// ---- finding things (not_found only) and checking versions (conflict, later) ---------------------------------

function team(s: OrgSnapshot, id: string): Team {
	return s.teams.find((x) => x.id === id) ?? fail('not_found', "That team isn't in the organization any more.");
}
function member(s: OrgSnapshot, id: string): Member {
	return s.members.find((x) => x.id === id) ?? fail('not_found', "That person isn't in the organization any more.");
}
function membership(s: OrgSnapshot, id: string): Membership {
	return s.memberships.find((x) => x.id === id) ?? fail('not_found', "That membership isn't there any more.");
}
const fresh = (found: { version: number }, expected: number, what: string) => {
	if (found.version !== expected) fail('conflict', `${what} changed since you looked.`);
};

// ---- invalid_request: the shape of what was sent --------------------------------------------------------------

const ID_MAX = 64;
const AGENT_TEXT_MAX = 200;
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;
export const badId = (id: string | undefined) => id !== undefined && (id === '' || [...id].length > ID_MAX || CONTROL.test(id));
const badText = (t: string | undefined) => t !== undefined && (trimWhiteSpace(t) === '' || [...t].length > AGENT_TEXT_MAX || CONTROL.test(t));

/** An id that is not an id can never name anything, so it is refused before the organization is looked at. */
export function wellFormed(c: OrgCommand) {
	const p = c.params as Record<string, unknown>;
	const idKeys = ['command_id', 'team_id', 'owner_team_id', 'lead_member_id', 'membership_id', 'new_membership_id', 'to_team_id', 'member_id', 'reports_to_member_id', 'project_team_id', 'project_id', 'office_id', 'area_id', 'general_area_id', 'areas_to_team_id'];
	const nested = Array.isArray(p.teams) ? (p.teams as { project_team_id?: string; team_id?: string }[]).flatMap((t) => [t.project_team_id, t.team_id]) : [];
	if (idKeys.some((k) => badId(p[k] as string | undefined)) || nested.some((id) => badId(id))) fail('invalid_request', "An id in that request isn't valid.");
}

/** A title or contact that is empty (clear it instead), too long or has control characters: in the command's own invalid_request step. */
const checkText = (t: string | undefined) => {
	if (badText(t)) fail('invalid_request', "A title or contact can't be empty, too long or contain control characters.");
};

/** The id of a new member is exactly 26 ASCII letters or digits (ULID-shaped); the sample's readable ids are display fixtures. */
function checkNewMemberId(id: string) {
	if (!/^[A-Za-z0-9]{26}$/.test(id)) fail('invalid_request', "A new member's id is 26 letters or digits.");
}

// ---- names ------------------------------------------------------------------------------------------------------

/** Whether a sibling of `parent` (a team with the same owner) already has this name, other than `exceptId`. */
const siblingTaken = (s: OrgSnapshot, parent: string | undefined, name: string, exceptId?: string) =>
	s.teams.some((t) => t.id !== exceptId && (t.owner_team_id ?? undefined) === parent && sameName(t.name, name));

/**
 * Whether `name` is another member's handle (one namespace, ignoring case): a person's name, an agent's
 * handle or the owner's reserved `owner`. The mock's agents carry no separate messaging name, so an agent's
 * handle is its id (as in the Rust mock's sample); `exceptId` is the member being renamed.
 */
const handleTaken = (s: OrgSnapshot, name: string, exceptId?: string) =>
	sameName(name, 'owner') ||
	s.members.some((m) => m.id !== exceptId && !m.owner && sameName(m.kind === 'agent' ? (m.agent?.messaging_name ?? m.id) : m.name, name));

const bump = <T extends { version: number }>(x: T): T => ({ ...x, version: x.version + 1 });
const replace = <T extends { id: string }>(xs: T[], next: T) => xs.map((x) => (x.id === next.id ? next : x));
const without = <T extends object, K extends keyof T>(x: T, key: K): Omit<T, K> => {
	const { [key]: _gone, ...rest } = x;
	return rest;
};

/** A team whose lead was `memberId` loses the lead (its version goes up). */
const clearLead = (teams: Team[], teamId: string, memberId: string): Team[] =>
	teams.map((t) => (t.id === teamId && t.lead_member_id === memberId ? bump(without(t, 'lead_member_id') as Team) : t));

const isOffboarded = (m: Member) => (m as Member & { agent?: { status?: string } }).agent?.status === 'offboarded';

// ---- an agent's profile (the contract's limits; invalid_request except a messaging name's format) ------------

const HOME_MAX_BYTES = 1024;
const TIME_ZONE_MAX = 64;
const bytes = (s: string) => new TextEncoder().encode(s).length;

/** Text as HQ stores it: trimmed, 1 to 200 characters, no control characters. */
function agentText(raw: string, what: string): string {
	const t = trimWhiteSpace(raw);
	if (t === '') fail('invalid_request', `${what} can't be empty.`);
	if ([...t].length > AGENT_TEXT_MAX) fail('invalid_request', `${what} can be at most ${AGENT_TEXT_MAX} characters.`);
	if (CONTROL.test(t)) fail('invalid_request', `${what} can't contain control characters.`);
	return t;
}

function messagingName(raw: string): string {
	if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(raw)) fail('invalid_name', 'A messaging name is 1 to 64 letters, digits, - _ or ., starting with a letter or digit.');
	return raw;
}

function agentRuntime(r: AgentRuntime | undefined): AgentRuntime {
	if (r === undefined || r === 'unknown') return fail('invalid_request', 'Choose what the agent runs on.');
	if (typeof r === 'object') {
		if (r === null || typeof (r as { other?: unknown }).other !== 'string') return fail('invalid_request', 'Choose what the agent runs on.');
		const name = agentText(r.other, "The runtime's name");
		if (['claude_code', 'codex', 'antigravity'].includes(name)) fail('invalid_request', 'Use the known runtime, not a copy of its name.');
		return { other: name };
	}
	if (!['claude_code', 'codex', 'antigravity'].includes(r)) fail('invalid_request', 'Choose what the agent runs on.');
	return r;
}

function agentHome(raw: string): string {
	if (raw === '' || bytes(raw) > HOME_MAX_BYTES || CONTROL.test(raw) || !raw.startsWith('/') || raw.split('/').includes('..'))
		fail('invalid_request', 'The home folder must be an absolute path (no .. parts, at most 1024 bytes).');
	return raw;
}

function agentSchedule(sc: Schedule): Schedule {
	const spec = agentText(sc?.spec ?? '', 'The schedule');
	const zone = sc?.time_zone ?? '';
	if (zone === '' || [...zone].length > TIME_ZONE_MAX || !zone.split('/').every((part) => /^[A-Za-z0-9_+-]+$/.test(part)))
		fail('invalid_request', 'The time zone must be a name like Europe/Lisbon.');
	return { spec, time_zone: zone };
}

/** A run mode with its schedule: scheduled needs one, any other mode must have none, unknown is refused. */
function agentRunMode(mode: RunMode | undefined, schedule: Schedule | undefined): { run_mode?: RunMode; schedule?: Schedule } {
	if (mode !== undefined && !['always_on', 'on_call', 'scheduled'].includes(mode)) fail('invalid_request', 'Choose how the agent is run.');
	if (mode === 'scheduled') {
		if (!schedule) fail('invalid_request', 'A scheduled agent needs a schedule.');
		return { run_mode: mode, schedule: agentSchedule(schedule!) };
	}
	if (schedule) fail('invalid_request', 'Only a scheduled agent has a schedule.');
	return mode === undefined ? {} : { run_mode: mode };
}

/** A whole profile as HQ stores it when an agent is added. */
function newProfile(p: AgentProfile): AgentProfile {
	const role = agentText(p.role, 'The role');
	const title = agentText(p.title, 'The title');
	const focus = p.focus !== undefined ? agentText(p.focus, 'The focus') : undefined;
	const model = p.model !== undefined ? agentText(p.model, 'The model') : undefined;
	const runtime = agentRuntime(p.runtime);
	const home = p.home !== undefined ? agentHome(p.home) : undefined;
	if (p.status !== 'active' && p.status !== 'paused') fail('invalid_request', 'A new agent is active or paused.');
	if (p.offboarded_at !== undefined) fail('invalid_request', "A new agent hasn't been offboarded.");
	const mode = agentRunMode(p.run_mode, p.schedule);
	const handle = messagingName(p.messaging_name);
	return { role, title, ...(focus !== undefined ? { focus } : {}), runtime, ...(model !== undefined ? { model } : {}), ...mode, ...(home !== undefined ? { home } : {}), messaging_name: handle, status: p.status };
}

/** What HQ knows besides the structure: its clock and which offices exist (undefined: not checked). */
export interface Ctx {
	now: string;
	offices?: string[];
}

/** Everything that happens to a member's place when they leave it (removed, or an agent offboarded). */
function detach(s: OrgSnapshot, id: string, manager: string | undefined): OrgSnapshot {
	const led = s.teams.filter((t) => t.lead_member_id === id).map((t) => t.id);
	const teams = led.reduce((acc, teamId) => clearLead(acc, teamId, id), s.teams);
	const members = s.members.map((x) => {
		if (x.reports_to_member_id !== id) return x;
		const rest = without(x, 'reports_to_member_id') as Member;
		return bump(manager ? { ...rest, reports_to_member_id: manager } : rest);
	});
	return { ...s, teams, members, memberships: s.memberships.filter((x) => x.member_id !== id) };
}

/** Apply a command. Returns the next snapshot with its position advanced by one. */
export function applyCommand(s: OrgSnapshot, given: OrgCommand, ctx: Ctx = { now: new Date().toISOString() }): OrgSnapshot {
	const command = withoutNulls(given);
	wellFormed(command);
	const next = reconcileAreas(apply(s, command, ctx));
	return { ...next, position: { epoch: s.position.epoch, seq: s.position.seq + 1 } };
}

// ---- projects and areas (design tasks-projects-docs.md §1.9, §2.5, §4.8) ------------------------------------------

const RESERVED_KEYS = ['req', 'feat', 'adr', 'eval', 'debt', 'lim', 'exc'];
const DESCRIPTION_MAX = 1000;
const AREA_NAME_MAX = 40;
const projectOf = (s: OrgSnapshot, id: string): ProjectSummary => (s.projects ?? []).find((x) => x.id === id) ?? fail('not_found', "That project isn't there any more.");
const areaOf = (s: OrgSnapshot, id: string): Area => (s.areas ?? []).find((x) => x.id === id) ?? fail('not_found', "That area isn't there any more.");
const keyOk = (key: string) => /^[a-z][a-z0-9]{1,9}$/.test(key);
const keyTaken = (s: OrgSnapshot, key: string) => RESERVED_KEYS.includes(key) || (s.projects ?? []).some((p) => p.key === key);
const projectNameTaken = (s: OrgSnapshot, name: string, except?: string) => (s.projects ?? []).some((p) => p.id !== except && sameName(p.name, name));
const areaNameTaken = (s: OrgSnapshot, projectId: string, name: string, except?: string) =>
	(s.areas ?? []).some((a) => a.project_id === projectId && !a.deleted && a.id !== except && sameName(a.name, name));
const projectTeamIds = (s: OrgSnapshot, projectId: string) => (s.project_teams ?? []).filter((l) => l.project_id === projectId).map((l) => l.team_id);

/** A key from the name: its ASCII letters and digits, lowercased, the first 3 (prefixed with `p` if that doesn't start with a letter, padded with `x` to 2); a digit 2, 3, … is appended while it is taken or reserved. */
export function deriveKey(s: OrgSnapshot, name: string): string {
	let base = [...name].filter((c) => /[A-Za-z0-9]/.test(c)).slice(0, 3).join('').toLowerCase();
	if (!/^[a-z]/.test(base)) base = ('p' + base).slice(0, 3);
	while (base.length < 2) base += 'x';
	let key = base;
	for (let n = 2; keyTaken(s, key); n++) key = `${base}${n}`;
	return key;
}

/** An area's name: the organization-name rules, and at most 40 characters. */
function checkedAreaName(raw: string): string {
	const name = checkedName(raw);
	if ([...name].length > AREA_NAME_MAX) fail('invalid_name', `An area's name can be at most ${AREA_NAME_MAX} characters.`);
	return name;
}

function checkDescription(text: string) {
	if ([...text].length > DESCRIPTION_MAX || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/.test(text)) fail('invalid_request', `A description can be at most ${DESCRIPTION_MAX} characters, without control characters.`);
}
function checkBranch(text: string) {
	if (trimWhiteSpace(text) === '' || [...text].length > 255 || CONTROL.test(text)) fail('invalid_request', "That isn't a branch name.");
}

/** A lead must be a live, non-owner member of the area's team (shared by `area.create` and `area.set_lead`). */
function checkLead(s: OrgSnapshot, leadId: string, teamId: string | undefined) {
	const m = member(s, leadId);
	if (isOffboarded(m)) fail('invalid_request', `${m.name} has left the organization.`);
	if (teamId === undefined) fail('invalid_request', 'An area needs a team before it has a lead.');
	if (m.owner) fail('owner_protected', "The owner can't lead an area.");
	if (!s.memberships.some((x) => x.team_id === teamId && x.member_id === leadId)) fail('not_a_member', `${m.name} isn't in that team.`);
}

/** Restore the area invariants after any command (design §2.5): an area's team is one of its project's teams (else the area is team-less) and its lead a live member of that team (else it has no lead). Each area that changes goes up one version. */
function reconcileAreas(s: OrgSnapshot): OrgSnapshot {
	if (!s.areas || s.areas.length === 0) return s;
	const areas = s.areas.map((a) => {
		if (a.deleted) return a;
		let next = a;
		if (next.team_id !== undefined && !(s.teams.some((t) => t.id === next.team_id) && (s.project_teams ?? []).some((l) => l.project_id === next.project_id && l.team_id === next.team_id))) next = without(next, 'team_id') as Area;
		if (next.lead_member_id !== undefined) {
			const lead = next.lead_member_id;
			const live = s.members.some((m) => m.id === lead && !isOffboarded(m));
			const inTeam = next.team_id !== undefined && s.memberships.some((m) => m.team_id === next.team_id && m.member_id === lead);
			if (!live || !inTeam) next = without(next, 'lead_member_id') as Area;
		}
		return next === a ? a : bump(next);
	});
	return areas.some((a, i) => a !== s.areas![i]) ? { ...s, areas } : s;
}

function apply(s: OrgSnapshot, c: OrgCommand, ctx: Ctx): OrgSnapshot {
	switch (c.method) {
		case 'team.create': {
			const p = c.params;
			if (p.owner_team_id !== undefined) team(s, p.owner_team_id);
			const name = checkedName(p.name);
			if (s.teams.some((t) => t.id === p.team_id)) fail('already_exists', 'That team id is taken.');
			if (siblingTaken(s, p.owner_team_id, name)) fail('already_exists', `There's already a team called ${name} here.`);
			return { ...s, teams: [...s.teams, { id: p.team_id, name, version: 1, ...(p.owner_team_id !== undefined ? { owner_team_id: p.owner_team_id } : {}) }] };
		}
		case 'team.rename': {
			const p = c.params;
			const t = team(s, p.team_id);
			const name = checkedName(p.name);
			fresh(t, p.expected_version, t.name);
			if (siblingTaken(s, t.owner_team_id, name, t.id)) fail('already_exists', `There's already a team called ${name} here.`);
			return { ...s, teams: replace(s.teams, bump({ ...t, name })) };
		}
		case 'team.move': {
			const p = c.params;
			const t = team(s, p.team_id);
			if (p.owner_team_id !== undefined) team(s, p.owner_team_id);
			if (p.owner_team_id !== undefined && (p.owner_team_id === t.id || teamsBelow(s, t.id).has(p.owner_team_id)))
				fail('cycle', `${t.name} can't go under itself or a team below it.`);
			fresh(t, p.expected_version, t.name);
			if (siblingTaken(s, p.owner_team_id, t.name, t.id)) fail('already_exists', `There's already a team called ${t.name} there.`);
			const moved = bump(p.owner_team_id !== undefined ? { ...t, owner_team_id: p.owner_team_id } : (without(t, 'owner_team_id') as Team));
			return { ...s, teams: replace(s.teams, moved) };
		}
		case 'team.set_lead': {
			const p = c.params;
			const t = team(s, p.team_id);
			const m = p.lead_member_id !== undefined ? member(s, p.lead_member_id) : undefined;
			if (m && isOffboarded(m)) fail('invalid_request', `${m.name} has left the organization.`);
			if (m?.owner) fail('owner_protected', "The owner can't lead a team.");
			if (m && !s.memberships.some((x) => x.team_id === t.id && x.member_id === m.id)) fail('not_a_member', `${m.name} isn't in ${t.name}.`);
			fresh(t, p.expected_version, t.name);
			const led = bump(m ? { ...t, lead_member_id: m.id } : (without(t, 'lead_member_id') as Team));
			return { ...s, teams: replace(s.teams, led) };
		}
		case 'team.delete': {
			const p = c.params;
			const t = team(s, p.team_id);
			if (s.teams.some((x) => x.owner_team_id === t.id)) fail('not_empty', `${t.name} still has teams under it.`);
			fresh(t, p.expected_version, t.name);
			return {
				...s,
				teams: s.teams.filter((x) => x.id !== t.id),
				memberships: s.memberships.filter((m) => m.team_id !== t.id),
				project_teams: (s.project_teams ?? []).filter((pt) => pt.team_id !== t.id)
			};
		}
		case 'team.add_member': {
			const p = c.params;
			const t = team(s, p.team_id);
			const m = member(s, p.member_id);
			checkText(p.title_in_team);
			if (isOffboarded(m)) fail('invalid_request', `${m.name} has left the organization.`);
			if (m.owner) fail('owner_protected', "The owner isn't on a team: they are in every team already.");
			if (s.memberships.some((x) => x.id === p.membership_id || (x.team_id === t.id && x.member_id === m.id)))
				fail('already_exists', `${m.name} is already in ${t.name}.`);
			const ms: Membership = { id: p.membership_id, team_id: t.id, member_id: m.id, version: 1, ...(p.title_in_team !== undefined ? { title_in_team: p.title_in_team } : {}) };
			return { ...s, memberships: [...s.memberships, ms] };
		}
		case 'team.remove_member': {
			const p = c.params;
			const ms = membership(s, p.membership_id);
			fresh(ms, p.expected_version, 'That membership');
			return { ...s, teams: clearLead(s.teams, ms.team_id, ms.member_id), memberships: s.memberships.filter((x) => x.id !== ms.id) };
		}
		case 'team.set_title': {
			const p = c.params;
			const ms = membership(s, p.membership_id);
			checkText(p.title_in_team);
			fresh(ms, p.expected_version, 'That membership');
			const titled = bump(p.title_in_team !== undefined ? { ...ms, title_in_team: p.title_in_team } : (without(ms, 'title_in_team') as Membership));
			return { ...s, memberships: replace(s.memberships, titled) };
		}
		case 'member.add_person': {
			const p = c.params;
			checkText(p.contact);
			checkNewMemberId(p.member_id);
			const name = checkedName(p.name);
			if (s.members.some((m) => m.id === p.member_id)) fail('already_exists', 'That person id is taken.');
			if (handleTaken(s, name)) fail('already_exists', `${name} is already taken as a name here.`);
			const person: Member = { id: p.member_id, kind: 'person', name, version: 1, ...(p.contact !== undefined ? { contact: p.contact } : {}) };
			return { ...s, members: [...s.members, person] };
		}
		case 'member.update_person': {
			const p = c.params;
			const m = member(s, p.member_id);
			checkText(p.contact);
			if (m.kind !== 'person' || (p.contact !== undefined && p.clear_contact)) fail('invalid_request', "That change doesn't apply to this member.");
			const renamed = p.name !== undefined ? checkedName(p.name) : undefined;
			fresh(m, p.expected_version, m.name);
			if (renamed !== undefined && !m.owner && handleTaken(s, renamed, m.id)) fail('already_exists', `${renamed} is already taken as a name here.`);
			let updated: Member = { ...m, ...(renamed !== undefined ? { name: renamed } : {}), ...(p.contact !== undefined ? { contact: p.contact } : {}) };
			if (p.clear_contact) updated = without(updated, 'contact') as Member;
			return { ...s, members: replace(s.members, bump(updated)) };
		}
		case 'member.remove': {
			const p = c.params;
			const m = member(s, p.member_id);
			if (isOffboarded(m)) fail('invalid_request', 'An agent who has left stays on record.');
			if (m.owner) fail('owner_protected', "The owner can't be removed.");
			fresh(m, p.expected_version, m.name);
			// Whoever reported to them now reports to their own manager, or to the owner if they had none.
			return detach({ ...s, members: s.members.filter((x) => x.id !== m.id) }, m.id, m.reports_to_member_id);
		}
		case 'member.set_reports_to': {
			const p = c.params;
			const m = member(s, p.member_id);
			const boss = p.reports_to_member_id !== undefined ? member(s, p.reports_to_member_id) : undefined;
			if (isOffboarded(m) || (boss && isOffboarded(boss))) fail('invalid_request', "Someone who has left can't be given a manager or be one.");
			if (m.owner) fail('owner_protected', 'The owner reports to no one.');
			if (boss && (boss.id === m.id || reportsBelow(s, m.id).has(boss.id))) fail('cycle', `${boss.name} reports to ${m.name}, directly or through others.`);
			fresh(m, p.expected_version, m.name);
			const to = boss && !boss.owner ? boss.id : undefined; // the owner's own id is stored as empty
			const updated = bump(to ? { ...m, reports_to_member_id: to } : (without(m, 'reports_to_member_id') as Member));
			return { ...s, members: replace(s.members, updated) };
		}
		case 'member.move': {
			const p = c.params;
			const old = membership(s, p.membership_id);
			const to = team(s, p.to_team_id);
			checkText(p.title_in_team);
			fresh(old, p.expected_version, 'That membership');
			if (s.memberships.some((x) => x.id === p.new_membership_id || (x.team_id === to.id && x.member_id === old.member_id)))
				fail('already_exists', `They are already in ${to.name}.`);
			const moved: Membership = { id: p.new_membership_id, team_id: to.id, member_id: old.member_id, version: 1, ...(p.title_in_team !== undefined ? { title_in_team: p.title_in_team } : {}) };
			return { ...s, teams: clearLead(s.teams, old.team_id, old.member_id), memberships: [...s.memberships.filter((x) => x.id !== old.id), moved] };
		}
		case 'member.add_agent': {
			const p = c.params;
			if (ctx.offices && !ctx.offices.includes(p.office_id)) fail('not_found', "That office isn't in the organization.");
			checkNewMemberId(p.member_id);
			const profile = newProfile(p.profile);
			const name = checkedName(p.name);
			if (s.members.some((m) => m.id === p.member_id)) fail('already_exists', 'That agent id is taken.');
			if (handleTaken(s, profile.messaging_name)) fail('already_exists', `${profile.messaging_name} is already taken as a name here.`);
			const agent: Member = { id: p.member_id, kind: 'agent', name, office_id: p.office_id, agent: profile, created_at: ctx.now, version: 1 };
			return { ...s, members: [...s.members, agent] };
		}
		case 'member.update_agent': {
			const p = c.params;
			const m = member(s, p.member_id);
			if (m.kind !== 'agent' || !m.agent) fail('invalid_request', 'That member isn\'t an agent: people are edited another way.');
			if (isOffboarded(m)) fail('invalid_request', "An agent who has left isn't edited: its record is history.");
			const given = [p.name, p.role, p.title, p.focus, p.runtime, p.model, p.home, p.messaging_name].some((v) => v !== undefined) || !!p.clear_focus || !!p.clear_model || !!p.clear_home;
			if (!given || (p.focus !== undefined && p.clear_focus) || (p.model !== undefined && p.clear_model) || (p.home !== undefined && p.clear_home))
				fail('invalid_request', 'Give a value to change, and not both a value and its clear flag.');
			let next: AgentProfile = { ...m.agent! };
			if (p.role !== undefined) next.role = agentText(p.role, 'The role');
			if (p.title !== undefined) next.title = agentText(p.title, 'The title');
			if (p.focus !== undefined) next.focus = agentText(p.focus, 'The focus');
			if (p.clear_focus) next = without(next, 'focus') as AgentProfile;
			if (p.runtime !== undefined) next.runtime = agentRuntime(p.runtime);
			if (p.model !== undefined) next.model = agentText(p.model, 'The model');
			if (p.clear_model) next = without(next, 'model') as AgentProfile;
			if (p.home !== undefined) next.home = agentHome(p.home);
			if (p.clear_home) next = without(next, 'home') as AgentProfile;
			const name = p.name !== undefined ? checkedName(p.name) : undefined;
			if (p.messaging_name !== undefined) next.messaging_name = messagingName(p.messaging_name);
			fresh(m, p.expected_version, m.name);
			if (p.messaging_name !== undefined && handleTaken(s, next.messaging_name, m.id)) fail('already_exists', `${next.messaging_name} is already taken as a name here.`);
			return { ...s, members: replace(s.members, bump({ ...m, ...(name !== undefined ? { name } : {}), agent: next })) };
		}
		case 'member.set_agent_status': {
			const p = c.params;
			const m = member(s, p.member_id);
			if (m.kind !== 'agent' || !m.agent) fail('invalid_request', "That member isn't an agent.");
			if (isOffboarded(m)) fail('invalid_request', "An agent who has left can't be paused or resumed.");
			if (p.status !== 'active' && p.status !== 'paused') fail('invalid_request', 'An agent is active or paused.');
			fresh(m, p.expected_version, m.name);
			return { ...s, members: replace(s.members, bump({ ...m, agent: { ...m.agent!, status: p.status } })) };
		}
		case 'member.set_run_mode': {
			const p = c.params;
			const m = member(s, p.member_id);
			if (m.kind !== 'agent' || !m.agent) fail('invalid_request', "That member isn't an agent.");
			if (isOffboarded(m)) fail('invalid_request', "An agent who has left isn't run.");
			const mode = agentRunMode(p.run_mode, p.schedule);
			fresh(m, p.expected_version, m.name);
			const rest = without(without(m.agent!, 'run_mode'), 'schedule') as AgentProfile;
			return { ...s, members: replace(s.members, bump({ ...m, agent: { ...rest, ...mode } })) };
		}
		case 'member.offboard': {
			const p = c.params;
			const m = member(s, p.member_id);
			if (m.kind !== 'agent' || !m.agent) fail('invalid_request', "That member isn't an agent: people are removed instead.");
			if (isOffboarded(m)) fail('invalid_request', 'That agent has already left.');
			fresh(m, p.expected_version, m.name);
			// Offboarding is a state, not a deletion: the record and its handle stay for good.
			const left = bump({ ...m, agent: { ...m.agent!, status: 'offboarded' as const, offboarded_at: ctx.now } });
			return detach({ ...s, members: replace(s.members, left) }, m.id, m.reports_to_member_id);
		}
		case 'project.add_team': {
			const p = c.params;
			team(s, p.team_id);
			if (!(s.projects ?? []).some((x) => x.id === p.project_id)) fail('not_found', "That project isn't there any more.");
			if ((s.project_teams ?? []).some((x) => x.id === p.project_team_id || (x.project_id === p.project_id && x.team_id === p.team_id)))
				fail('already_exists', 'That team already works on this project.');
			const link: ProjectTeam = { id: p.project_team_id, project_id: p.project_id, team_id: p.team_id, version: 1 };
			return { ...s, project_teams: [...(s.project_teams ?? []), link] };
		}
		case 'project.remove_team': {
			const p = c.params;
			const link = (s.project_teams ?? []).find((x) => x.id === p.project_team_id) ?? fail('not_found', "That link isn't there any more.");
			// The leaving team's areas go to `areas_to_team_id` when the project keeps other teams (required then), or are
			// left team-less when it was the last one (then it may not be given).
			const owned = (s.areas ?? []).filter((a) => a.project_id === link.project_id && !a.deleted && a.team_id === link.team_id);
			const remaining = (s.project_teams ?? []).filter((x) => x.project_id === link.project_id && x.id !== link.id).map((x) => x.team_id);
			if (p.areas_to_team_id !== undefined && (owned.length === 0 || remaining.length === 0 || !remaining.includes(p.areas_to_team_id)))
				fail('invalid_request', 'Areas can only go to another team that still works on the project, and only when the team owns some.');
			if (p.areas_to_team_id === undefined && owned.length > 0 && remaining.length > 0) fail('invalid_request', "Say which of the project's other teams takes over this team's areas.");
			fresh(link, p.expected_version, 'That link');
			const target = p.areas_to_team_id;
			// A lead who isn't in the team taking over stops leading, in the same change.
			const areas = (s.areas ?? []).map((a) => {
				if (target === undefined || !owned.some((o) => o.id === a.id)) return a;
				const moved: Area = { ...a, team_id: target };
				const stays = a.lead_member_id !== undefined && s.memberships.some((m) => m.team_id === target && m.member_id === a.lead_member_id);
				return bump(a.lead_member_id !== undefined && !stays ? (without(moved, 'lead_member_id') as Area) : moved);
			});
			return { ...s, project_teams: (s.project_teams ?? []).filter((x) => x.id !== link.id), areas };
		}
		case 'project.create': {
			const p = c.params;
			for (const t of p.teams ?? []) team(s, t.team_id);
			if (p.key !== undefined && (!keyOk(p.key) || RESERVED_KEYS.includes(p.key))) fail('invalid_request', 'A key is 2 to 10 lowercase letters and digits, starting with a letter, and not reserved.');
			if (p.description !== undefined) checkDescription(p.description);
			if (p.integration_branch !== undefined) checkBranch(p.integration_branch);
			const teamIds = (p.teams ?? []).map((t) => t.team_id);
			if (new Set(teamIds).size !== teamIds.length) fail('invalid_request', 'A team can be listed once.');
			let repo: ProjectSummary['repo'];
			if (p.repo && 'new_local' in p.repo) repo = { host: 'local', default_branch: 'main', path: `/workspace/repos/${p.project_id}.git` };
			else if (p.repo && 'existing_local' in p.repo) {
				const path = p.repo.existing_local.path;
				if (!path.startsWith('/') || CONTROL.test(path)) fail('invalid_request', 'A repository path is absolute.');
				repo = { host: 'local', default_branch: 'main', path };
			} else if (p.repo) fail('invalid_request', "That repository kind isn't supported yet.");
			const name = checkedName(p.name);
			const generalName = checkedAreaName(p.general_area_name ?? 'General');
			const linkIds = (p.teams ?? []).map((t) => t.project_team_id);
			if (new Set(linkIds).size !== linkIds.length) fail('already_exists', 'A link id is used twice.');
			if ((s.projects ?? []).some((x) => x.id === p.project_id) || (s.areas ?? []).some((a) => a.id === p.general_area_id) || (p.teams ?? []).some((t) => (s.project_teams ?? []).some((l) => l.id === t.project_team_id)))
				fail('already_exists', 'One of those ids is taken.');
			if (p.key !== undefined && keyTaken(s, p.key)) fail('already_exists', 'That key is taken.');
			if (projectNameTaken(s, name)) fail('already_exists', `There's already a project called ${name}.`);
			const project: ProjectSummary = {
				id: p.project_id,
				key: p.key ?? deriveKey(s, name),
				name,
				...(p.description !== undefined ? { description: p.description } : {}),
				...(repo ? { repo } : {}),
				...(p.integration_branch !== undefined ? { integration_branch: p.integration_branch } : {}),
				created_at: ctx.now,
				version: 1
			};
			const general: Area = { id: p.general_area_id, project_id: p.project_id, name: generalName, ...(teamIds[0] ? { team_id: teamIds[0] } : {}), general: true, version: 1 };
			const links: ProjectTeam[] = (p.teams ?? []).map((t) => ({ id: t.project_team_id, project_id: p.project_id, team_id: t.team_id, version: 1 }));
			return { ...s, projects: [...(s.projects ?? []), project], areas: [...(s.areas ?? []), general], project_teams: [...(s.project_teams ?? []), ...links] };
		}
		case 'project.update': {
			const p = c.params;
			const pr = projectOf(s, p.project_id);
			if ((p.description !== undefined && p.clear_description) || (p.integration_branch !== undefined && p.clear_integration_branch)) fail('invalid_request', 'Give a value or clear it, not both.');
			if (p.name === undefined && p.description === undefined && !p.clear_description && p.integration_branch === undefined && !p.clear_integration_branch) fail('invalid_request', 'Nothing to change.');
			if (p.description !== undefined) checkDescription(p.description);
			if (p.integration_branch !== undefined) checkBranch(p.integration_branch);
			const name = p.name !== undefined ? checkedName(p.name) : undefined;
			fresh({ version: pr.version ?? 0 }, p.expected_version, pr.name);
			if (name !== undefined && projectNameTaken(s, name, pr.id)) fail('already_exists', `There's already a project called ${name}.`);
			let next: ProjectSummary = { ...pr };
			if (name !== undefined) next.name = name;
			if (p.description !== undefined) next.description = p.description;
			if (p.clear_description) next = without(next, 'description') as ProjectSummary;
			if (p.integration_branch !== undefined) next.integration_branch = p.integration_branch;
			if (p.clear_integration_branch) next = without(next, 'integration_branch') as ProjectSummary;
			next.version = (pr.version ?? 0) + 1;
			return { ...s, projects: (s.projects ?? []).map((x) => (x.id === pr.id ? next : x)) };
		}
		case 'project.archive': {
			const p = c.params;
			const pr = projectOf(s, p.project_id);
			fresh({ version: pr.version ?? 0 }, p.expected_version, pr.name);
			return { ...s, projects: (s.projects ?? []).map((x) => (x.id === pr.id ? { ...x, archived: p.archived, version: (pr.version ?? 0) + 1 } : x)) };
		}
		case 'area.create': {
			const p = c.params;
			const pr = projectOf(s, p.project_id);
			if (p.team_id !== undefined) team(s, p.team_id);
			if (p.lead_member_id !== undefined) member(s, p.lead_member_id);
			const teams = projectTeamIds(s, pr.id);
			if (p.team_id !== undefined && !teams.includes(p.team_id)) fail('invalid_request', "That team doesn't work on this project.");
			if (p.team_id === undefined && teams.length > 0) fail('invalid_request', "Choose which of the project's teams owns this area.");
			if (p.lead_member_id !== undefined) {
				const m = member(s, p.lead_member_id);
				if (isOffboarded(m) || p.team_id === undefined) checkLead(s, p.lead_member_id, p.team_id);
			}
			const name = checkedAreaName(p.name);
			if (p.lead_member_id !== undefined) checkLead(s, p.lead_member_id, p.team_id);
			if ((s.areas ?? []).some((a) => a.id === p.area_id)) fail('already_exists', 'That area id is taken.');
			if (areaNameTaken(s, pr.id, name)) fail('already_exists', `${pr.name} already has an area called ${name}.`);
			const area: Area = { id: p.area_id, project_id: pr.id, name, ...(p.team_id !== undefined ? { team_id: p.team_id } : {}), ...(p.lead_member_id !== undefined ? { lead_member_id: p.lead_member_id } : {}), version: 1 };
			return { ...s, areas: [...(s.areas ?? []), area] };
		}
		case 'area.rename': {
			const p = c.params;
			const a = areaOf(s, p.area_id);
			if (a.deleted) fail('invalid_request', 'That area was deleted.');
			const name = checkedAreaName(p.name);
			fresh(a, p.expected_version, a.name);
			if (areaNameTaken(s, a.project_id, name, a.id)) fail('already_exists', `There's already an area called ${name} in this project.`);
			return { ...s, areas: replace(s.areas ?? [], bump({ ...a, name })) };
		}
		case 'area.set_team': {
			const p = c.params;
			const a = areaOf(s, p.area_id);
			if (p.team_id !== undefined) team(s, p.team_id);
			if (a.deleted) fail('invalid_request', 'That area was deleted.');
			const teams = projectTeamIds(s, a.project_id);
			if (p.team_id !== undefined && !teams.includes(p.team_id)) fail('invalid_request', "That team doesn't work on this project.");
			if (p.team_id === undefined && teams.length > 0) fail('invalid_request', "Choose which of the project's teams owns this area.");
			fresh(a, p.expected_version, a.name);
			let next: Area = p.team_id !== undefined ? { ...a, team_id: p.team_id } : (without(a, 'team_id') as Area);
			// A lead who isn't in the new team stops leading.
			if (next.lead_member_id && !(p.team_id !== undefined && s.memberships.some((m) => m.team_id === p.team_id && m.member_id === next.lead_member_id))) next = without(next, 'lead_member_id') as Area;
			return { ...s, areas: replace(s.areas ?? [], bump(next)) };
		}
		case 'area.set_lead': {
			const p = c.params;
			const a = areaOf(s, p.area_id);
			if (p.lead_member_id !== undefined) member(s, p.lead_member_id);
			if (a.deleted) fail('invalid_request', 'That area was deleted.');
			if (p.lead_member_id !== undefined) checkLead(s, p.lead_member_id, a.team_id);
			fresh(a, p.expected_version, a.name);
			const next = p.lead_member_id !== undefined ? { ...a, lead_member_id: p.lead_member_id } : (without(a, 'lead_member_id') as Area);
			return { ...s, areas: replace(s.areas ?? [], bump(next)) };
		}
	}
}

export { owner };
