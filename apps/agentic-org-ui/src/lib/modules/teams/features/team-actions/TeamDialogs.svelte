<!--
  @component
  The dialogs behind a team's menu (§7.2, §7.5, §7.7, §7.10, TEAM-15), shared by the Teams index and a team's
  page: New team (top level or under a team), Move to…, Rename…, Change lead…, Archive… and Delete….
  `open(action, team)` and `openNew(parentId?)` start one. Archive and Delete first check what still points at the
  team and refuse in the dialog while anything does (§7.10, TEAM-9). After a confirmed change it calls `reload` and
  says so; a removed or created team goes to the caller, which knows where focus belongs.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import NewTeamSheet from '$lib/modules/organization/features/teams/NewTeamSheet.svelte';
	import MoveTeamDialog from '$lib/modules/organization/features/teams/MoveTeamDialog.svelte';
	import RenameTeamDialog from '$lib/modules/organization/features/teams/RenameTeamDialog.svelte';
	import { membersApi } from '$lib/modules/members';
	import { toAppError } from '$lib/proto';
	import type { OrgSession } from '$lib/modules/organization/session';
	import { deleteSummary } from '$lib/modules/organization/team-edit';
	import type { OrgSnapshot } from '$lib/proto';
	import { teamsApi } from '../../api';
	import { teamBlockers, type BlockerCheck, type TeamRemoval } from '../../blockers';
	import type { LeadCandidate, TeamAction, TeamRow } from '../../types';
	import RemoveTeamDialog from '../remove-team/RemoveTeamDialog.svelte';
	import ChangeLeadDialog from '../change-lead/ChangeLeadDialog.svelte';

	let {
		organizationId,
		snapshot,
		session,
		reload,
		onremoved,
		oncreated
	}: {
		organizationId: string;
		snapshot: OrgSnapshot;
		session: OrgSession;
		/** Re-reads what the caller shows after a confirmed change. */
		reload: () => Promise<void>;
		/** A team was archived or deleted: the caller moves focus and confirms it. */
		onremoved: (removed: { action: 'archive' | 'delete'; teamId: string; name: string }) => void;
		oncreated: (created: { teamId: string; name: string; problems: string[] }) => void;
	} = $props();

	let target = $state<TeamRow | undefined>();
	let current = $state<TeamAction | 'new' | undefined>();
	let newUnder = $state<string | undefined>();
	let candidates = $state<LeadCandidate[] | undefined>();
	let removal = $state<TeamRemoval>('archive');
	let check = $state.raw<BlockerCheck>({ state: 'checking' });
	let checkRun = 0;
	const orgTeam = $derived(target && snapshot.teams.find((t) => t.id === target!.id));
	// Delete's other effects once nothing blocks it: the projects it only contributes to stay with their owners.
	const effects = $derived(
		orgTeam
			? deleteSummary(snapshot, orgTeam).lines.filter((l) => !l.startsWith('No one is in'))
			: []
	);

	/** Opens one dialog with the selected bind: the others stay closed (one overlay at a time). */
	const isOpen = (which: TeamAction | 'new') => ({
		get: () => current === which,
		set: (next: boolean) => (current = next ? which : undefined)
	});
	const newSheet = isOpen('new');
	const move = isOpen('move');
	const rename = isOpen('rename');
	const lead = isOpen('lead');
	/** Archive and Delete share one dialog; `removal` says which. */
	const removing = {
		get: () => current === 'archive' || current === 'delete',
		set: (next: boolean) => {
			if (!next) current = undefined;
		}
	};

	async function loadCandidates(teamId: string) {
		candidates = undefined;
		try {
			const everyone = await membersApi.directory(organizationId);
			candidates = everyone
				.filter((m) => m.homeTeam?.id === teamId && m.status !== 'inactive')
				.map((m) => ({ id: m.id, name: m.name, kind: m.kind }));
		} catch (e) {
			candidates = [];
			toast.error(`Couldn't load who can lead: ${toAppError(e).message}`);
		}
	}

	/** What still points at `teamId`, read fresh each time Archive or Delete is chosen or the org changes (§7.10, TEAM-9). */
	async function checkBlockers(teamId: string, kind: TeamRemoval, quiet = false) {
		const run = ++checkRun;
		if (!quiet) check = { state: 'checking' };
		try {
			const [teams, members, projects, work] = await Promise.all([
				teamsApi.list(organizationId),
				membersApi.directory(organizationId),
				teamsApi.projects(organizationId, teamId),
				teamsApi.work(organizationId, teamId)
			]);
			if (run === checkRun)
				check = {
					state: 'done',
					blockers: teamBlockers(teamId, { teams, members, projects, work }, kind)
				};
		} catch (e) {
			if (run === checkRun) check = { state: 'failed', message: toAppError(e).message };
		}
	}

	// Live updates apply in place while Archive or Delete is open (offices-and-teams-ux §7.10).
	let seenPosition = untrack(() => `${snapshot.position.epoch}:${snapshot.position.seq}`);
	$effect(() => {
		const pos = `${snapshot.position.epoch}:${snapshot.position.seq}`;
		if (pos === seenPosition) return;
		seenPosition = pos;
		const active = untrack(() => current);
		const team = untrack(() => target);
		if ((active === 'archive' || active === 'delete') && team)
			untrack(() => void checkBlockers(team.id, active, true));
	});

	/** Opens New team, at the top level or under `parentId`. */
	export function openNew(parentId?: string) {
		newUnder = parentId;
		current = 'new';
	}

	export function open(action: TeamAction, team: TeamRow) {
		target = team;
		if (action === 'add-team') return openNew(team.id);
		if (action === 'lead') void loadCandidates(team.id);
		if (action === 'archive' || action === 'delete') {
			removal = action;
			void checkBlockers(team.id, action);
		}
		current = action;
	}
</script>

<NewTeamSheet
	bind:open={newSheet.get, newSheet.set}
	{snapshot}
	{session}
	ownerTeamId={newUnder}
	ondone={oncreated}
/>
<MoveTeamDialog
	bind:open={move.get, move.set}
	team={orgTeam}
	{snapshot}
	{session}
	ondone={async (r) => {
		await reload();
		if (!('stale' in r)) toast.success(`Moved ${r.name} to ${r.to}.`);
	}}
/>
<RenameTeamDialog
	bind:open={rename.get, rename.set}
	team={orgTeam}
	{snapshot}
	{session}
	ondone={async (r) => {
		await reload();
		if (r.name) toast.success(`Renamed to ${r.name}.`);
	}}
/>
<RemoveTeamDialog
	bind:open={removing.get, removing.set}
	{removal}
	team={target}
	{organizationId}
	{check}
	{effects}
	onrecheck={() => target && checkBlockers(target.id, removal)}
	ondone={(r) => onremoved({ action: removal, teamId: target!.id, name: r.name })}
/>
<ChangeLeadDialog
	bind:open={lead.get, lead.set}
	team={target}
	{candidates}
	{session}
	ondone={async (r) => {
		await reload();
		toast.success(
			r.lead
				? `${r.lead} now leads ${r.team} and manages its subtree.`
				: `${r.team} has no lead now.`
		);
	}}
/>
