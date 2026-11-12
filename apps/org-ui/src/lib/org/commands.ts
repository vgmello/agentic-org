// The organization commands (crates/org-proto/src/org.rs): what the UI asks HQ to change. Every command
// carries a client-made `command_id` (a ULID; a retry with the same id gets the same answer) and
// client-made ids for anything it creates, and the `expected_version` of what it edits.

const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

/** A new ULID: 10 characters of time (ms) and 16 of randomness, in Crockford base32. */
export function newId(now: number = Date.now(), random: () => number = Math.random): string {
	let time = '';
	for (let t = now, i = 0; i < 10; i++, t = Math.floor(t / 32)) time = CROCKFORD[t % 32] + time;
	const bytes = new Uint8Array(16);
	if (globalThis.crypto?.getRandomValues) globalThis.crypto.getRandomValues(bytes);
	else bytes.forEach((_, i) => (bytes[i] = Math.floor(random() * 256)));
	let rand = '';
	for (const b of bytes) rand += CROCKFORD[b % 32];
	return time + rand;
}

import type { AgentProfile, AgentRuntime, AgentStatus, RunMode, Schedule } from '$lib/proto';

type C<M extends string, P> = { method: M; params: { command_id: string } & P };

export type OrgCommand =
	| C<'team.create', { team_id: string; name: string; owner_team_id?: string }>
	| C<'team.rename', { team_id: string; expected_version: number; name: string }>
	| C<'team.move', { team_id: string; expected_version: number; owner_team_id?: string }>
	| C<'team.set_lead', { team_id: string; expected_version: number; lead_member_id?: string }>
	| C<'team.delete', { team_id: string; expected_version: number }>
	| C<'team.add_member', { membership_id: string; team_id: string; member_id: string; title_in_team?: string }>
	| C<'team.remove_member', { membership_id: string; expected_version: number }>
	| C<'team.set_title', { membership_id: string; expected_version: number; title_in_team?: string }>
	| C<'member.add_person', { member_id: string; name: string; contact?: string }>
	| C<'member.update_person', { member_id: string; expected_version: number; name?: string; contact?: string; clear_contact?: boolean }>
	| C<'member.remove', { member_id: string; expected_version: number }>
	| C<'member.set_reports_to', { member_id: string; expected_version: number; reports_to_member_id?: string }>
	| C<'member.move', { membership_id: string; expected_version: number; new_membership_id: string; to_team_id: string; title_in_team?: string }>
	| C<'member.add_agent', { member_id: string; name: string; office_id: string; profile: AgentProfile }>
	| C<
			'member.update_agent',
			{
				member_id: string;
				expected_version: number;
				name?: string;
				role?: string;
				title?: string;
				focus?: string;
				clear_focus?: boolean;
				runtime?: AgentRuntime;
				model?: string;
				clear_model?: boolean;
				home?: string;
				clear_home?: boolean;
				messaging_name?: string;
			}
	  >
	| C<'member.set_agent_status', { member_id: string; expected_version: number; status: AgentStatus }>
	| C<'member.set_run_mode', { member_id: string; expected_version: number; run_mode?: RunMode; schedule?: Schedule }>
	| C<'member.offboard', { member_id: string; expected_version: number }>
	| C<'project.add_team', { project_team_id: string; project_id: string; team_id: string }>
	| C<'project.remove_team', { project_team_id: string; expected_version: number; areas_to_team_id?: string }>
	| C<
			'project.create',
			{
				project_id: string;
				name: string;
				key?: string;
				description?: string;
				teams?: { project_team_id: string; team_id: string }[];
				general_area_id: string;
				general_area_name?: string;
				repo?: { new_local: Record<string, never> } | { existing_local: { path: string } };
				integration_branch?: string;
			}
	  >
	| C<'project.update', { project_id: string; expected_version: number; name?: string; description?: string; clear_description?: boolean; integration_branch?: string; clear_integration_branch?: boolean }>
	| C<'project.archive', { project_id: string; expected_version: number; archived: boolean }>
	| C<'area.create', { area_id: string; project_id: string; name: string; team_id?: string; lead_member_id?: string }>
	| C<'area.rename', { area_id: string; expected_version: number; name: string }>
	| C<'area.set_team', { area_id: string; expected_version: number; team_id?: string }>
	| C<'area.set_lead', { area_id: string; expected_version: number; lead_member_id?: string }>;

export type OrgMethod = OrgCommand['method'];

type Without<T, K extends string> = T extends unknown ? Omit<T, K> : never;
/** A command without its `command_id`: callers say what they want; the API adds the id. */
export type OrgCommandInput = { [M in OrgMethod]: { method: M; params: Without<Extract<OrgCommand, { method: M }>['params'], 'command_id'> } }[OrgMethod];

/** `null` means "not given", as it does on the wire (an Option): drop it so `{ lead_member_id: null }` clears a lead. */
export function withoutNulls<T extends OrgCommand | OrgCommandInput>(command: T): T {
	const clean = (o: Record<string, unknown>) => Object.fromEntries(Object.entries(o).filter(([, v]) => v !== null));
	const params = clean(command.params as Record<string, unknown>);
	// An agent's profile (member.add_agent) and a schedule are nested: their nulls mean "not given" too.
	for (const key of ['profile', 'schedule']) if (params[key] && typeof params[key] === 'object') params[key] = clean(params[key] as Record<string, unknown>);
	return { ...command, params } as T;
}

/** A command ready to send: with the `command_id` it already has, or a new one, and no nulls. */
export function ready(input: OrgCommandInput | OrgCommand): OrgCommand {
	const clean = withoutNulls(input);
	return 'command_id' in clean.params ? (clean as OrgCommand) : withId(clean as OrgCommandInput);
}

/** Give a command its client-made id. */
export function withId(input: OrgCommandInput, id: string = newId()): OrgCommand {
	return { method: input.method, params: { command_id: id, ...input.params } } as OrgCommand;
}
