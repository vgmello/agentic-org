import { isBuiltInAgent, toAppError } from '$lib/proto';
import type { AgentRuntime, AppError, Member, RunMode, Schedule } from '$lib/proto';
import { newStableId } from './commands';
import type { OrgSession } from './session';

export interface StepResult {
	/** What was being done, in words (for "Couldn't add Noor to Web: …"). */
	what: string;
	error?: AppError;
}

export async function attempt(what: string, run: () => Promise<unknown>): Promise<StepResult> {
	try {
		await run();
		return { what };
	} catch (e) {
		return { what, error: toAppError(e) };
	}
}

export function assertMutableAgent(member: Member, message: string): void {
	if (isBuiltInAgent(member)) {
		throw { source: 'api', code: 'not_authorized', message } satisfies AppError;
	}
}

export interface NewAgentInput {
	name: string;
	officeId: string;
	role: string;
	title: string;
	focus?: string;
	runtime: AgentRuntime;
	model?: string;
	runMode?: RunMode;
	home?: string;
	messagingName: string;
	status?: 'active' | 'paused';
	teamIds: string[];
	managerId?: string;
}

export interface NewAgentResult {
	memberId: string;
	created: StepResult;
	steps: StepResult[];
}

/** Add an agent (spec §7, "Add an existing agent"), then its teams and manager, each its own command. Never throws. */
export async function addAgent(
	api: OrgSession,
	input: NewAgentInput,
	teamName: (id: string) => string = (id) => id
): Promise<NewAgentResult> {
	const memberId = newStableId();
	const created = await attempt(`Add ${input.name}`, () =>
		api.command({
			method: 'member.add_agent',
			params: {
				member_id: memberId,
				name: input.name,
				office_id: input.officeId,
				profile: {
					role: input.role,
					title: input.title,
					...(input.focus ? { focus: input.focus } : {}),
					runtime: input.runtime,
					...(input.model ? { model: input.model } : {}),
					...(input.runMode ? { run_mode: input.runMode } : {}),
					...(input.home ? { home: input.home } : {}),
					messaging_name: input.messagingName,
					status: input.status ?? 'active'
				}
			}
		})
	);
	if (created.error) return { memberId, created, steps: [] };
	const steps: StepResult[] = [];
	for (const teamId of input.teamIds) {
		steps.push(
			await attempt(`Add ${input.name} to ${teamName(teamId)}`, async () => {
				const me = (await api.snapshot()).members.find((m) => m.id === memberId);
				if (!me)
					throw {
						source: 'api',
						code: 'not_found',
						message: "That agent isn't in the organization any more."
					} satisfies AppError;
				await api.command({
					method: 'member.move',
					params: {
						membership_id: me.id,
						expected_version: me.version,
						new_membership_id: newStableId(),
						to_team_id: teamId
					}
				});
			})
		);
	}
	if (input.managerId) {
		const me = (await api.snapshot()).members.find((m) => m.id === memberId);
		if (me)
			steps.push(
				await attempt('Set their manager', () =>
					api.command({
						method: 'member.set_reports_to',
						params: {
							member_id: memberId,
							expected_version: me.version,
							reports_to_member_id: input.managerId
						}
					})
				)
			);
	}
	return { memberId, created, steps };
}

/**
 * The member as HQ has it now, or, when the caller saw a particular version (a dialog showed what it would do), that
 * version: HQ then refuses with `conflict` if the agent changed since, instead of acting on what the owner didn't see.
 */
async function currentMember(api: OrgSession, memberId: string, seenVersion?: number) {
	const m = (await api.snapshot()).members.find((x) => x.id === memberId);
	if (!m)
		throw {
			source: 'api',
			code: 'not_found',
			message: "That agent isn't in the organization any more."
		} satisfies AppError;
	return seenVersion === undefined ? m : { ...m, version: seenVersion };
}

/** Pause or resume an agent, against its current version (DEAL-3). A paused agent isn't started or woken. */
export async function setAgentStatus(
	api: OrgSession,
	memberId: string,
	status: 'active' | 'paused',
	seenVersion?: number
): Promise<void> {
	const m = await currentMember(api, memberId, seenVersion);
	assertMutableAgent(m, 'Built-in agents cannot be paused or resumed.');
	await api.command({
		method: 'member.set_agent_status',
		params: { member_id: memberId, expected_version: m.version, status }
	});
}

/** Offboard an agent: a state, not a deletion (its record and name stay). */
export async function offboardAgent(
	api: OrgSession,
	memberId: string,
	seenVersion?: number
): Promise<void> {
	const m = await currentMember(api, memberId, seenVersion);
	assertMutableAgent(m, 'Built-in agents cannot be offboarded.');
	await api.command({
		method: 'member.offboard',
		params: { member_id: memberId, expected_version: m.version }
	});
}

export interface AgentChange {
	name?: string;
	role?: string;
	title?: string;
	/** Empty clears it. */
	focus?: string;
	runtime?: AgentRuntime;
	model?: string;
	home?: string;
	messagingName?: string;
}

/** Edit an agent's record: only what is given changes, and an empty focus, model or home clears it. */
export async function updateAgent(
	api: OrgSession,
	memberId: string,
	change: AgentChange,
	seenVersion?: number
): Promise<void> {
	const m = await currentMember(api, memberId, seenVersion);
	if ((change.name ?? change.messagingName ?? change.role) !== undefined)
		assertMutableAgent(m, 'Built-in agents cannot change name, role, or handle.');
	const clear = (v: string | undefined) => v !== undefined && v.trim() === '';
	const given = (v: string | undefined) => (v !== undefined && v.trim() !== '' ? v : undefined);
	await api.command({
		method: 'member.update_agent',
		params: {
			member_id: memberId,
			expected_version: m.version,
			...(change.name !== undefined ? { name: change.name } : {}),
			...(change.role !== undefined ? { role: change.role } : {}),
			...(change.title !== undefined ? { title: change.title } : {}),
			...(given(change.focus) ? { focus: change.focus } : {}),
			...(clear(change.focus) ? { clear_focus: true } : {}),
			...(change.runtime !== undefined ? { runtime: change.runtime } : {}),
			...(given(change.model) ? { model: change.model } : {}),
			...(clear(change.model) ? { clear_model: true } : {}),
			...(given(change.home) ? { home: change.home } : {}),
			...(clear(change.home) ? { clear_home: true } : {}),
			...(change.messagingName !== undefined ? { messaging_name: change.messagingName } : {})
		}
	});
}

/** Change how an agent runs (RUN-1): a mode, with a schedule for a scheduled one; none means a person starts it. */
export async function setRunMode(
	api: OrgSession,
	memberId: string,
	mode: RunMode | undefined,
	schedule?: Schedule,
	seenVersion?: number
): Promise<void> {
	const m = await currentMember(api, memberId, seenVersion);
	await api.command({
		method: 'member.set_run_mode',
		params: {
			member_id: memberId,
			expected_version: m.version,
			...(mode ? { run_mode: mode } : {}),
			...(mode === 'scheduled' && schedule ? { schedule } : {})
		}
	});
}

/**
 * Move a member from their current home team to another in one step (`member.move`): a lead who leaves the old team
 * clears its lead. Returns how to undo it (the reverse move). The old title isn't carried over.
 */
export async function moveMemberToTeam(
	api: OrgSession,
	memberId: string,
	fromTeamId: string,
	toTeamId: string
): Promise<{ undo: () => Promise<void>; lostLead: boolean }> {
	const before = await api.snapshot();
	const current = before.members.find((m) => m.id === memberId && m.home_team_id === fromTeamId);
	const lostLead = before.teams.find((t) => t.id === fromTeamId)?.lead_member_id === memberId;
	if (!current)
		throw {
			source: 'api',
			code: 'not_found',
			message: "They aren't in that team any more."
		} satisfies AppError;
	assertMutableAgent(current, 'Built-in agents cannot be moved to another home team.');
	await api.command({
		method: 'member.move',
		params: {
			membership_id: current.id,
			expected_version: current.version,
			new_membership_id: newStableId(),
			to_team_id: toTeamId
		}
	});
	return {
		lostLead,
		undo: async () => {
			const now = (await api.snapshot()).members.find(
				(m) => m.id === memberId && m.home_team_id === toTeamId
			);
			// Moved again, removed or offboarded since: say so instead of reporting a success that didn't happen.
			if (!now)
				throw {
					source: 'api',
					code: 'not_found',
					message: "They aren't in that team any more."
				} satisfies AppError;
			await api.command({
				method: 'member.move',
				params: {
					membership_id: now.id,
					expected_version: now.version,
					new_membership_id: newStableId(),
					to_team_id: fromTeamId
				}
			});
		}
	};
}
