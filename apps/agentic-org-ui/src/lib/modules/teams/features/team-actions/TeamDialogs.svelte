<!--
  @component
  The dialogs behind a team's menu (§7.2, §7.5, §7.7, §7.10, TEAM-15), shared by the Teams index and a team's
  page: New team (top level or under a team), Move to…, Rename…, Change lead…, Archive… and Delete….
  `open(action, team)` and `openNew(parentId?)` start one. After a confirmed change it calls `reload` and says
  so; a removed or created team goes to the caller, which knows where focus belongs.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import NewTeamSheet from '$lib/components/NewTeamSheet.svelte';
	import MoveTeamDialog from '$lib/components/MoveTeamDialog.svelte';
	import RenameTeamDialog from '$lib/components/RenameTeamDialog.svelte';
	import DeleteTeamDialog from '$lib/components/DeleteTeamDialog.svelte';
	import { membersApi } from '$lib/modules/members';
	import { toAppError } from '$lib/office/api';
	import type { OrgSession } from '$lib/org/session';
	import type { OrgSnapshot } from '$lib/proto';
	import type { LeadCandidate, TeamAction, TeamRow } from '../../types';
	import ArchiveTeamDialog from '../archive-team/ArchiveTeamDialog.svelte';
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
	const orgTeam = $derived(target && snapshot.teams.find((t) => t.id === target!.id));

	/** Opens one dialog with the selected bind: the others stay closed (one overlay at a time). */
	const isOpen = (which: TeamAction | 'new') => ({
		get: () => current === which,
		set: (next: boolean) => (current = next ? which : undefined)
	});
	const newSheet = isOpen('new');
	const move = isOpen('move');
	const rename = isOpen('rename');
	const lead = isOpen('lead');
	const archive = isOpen('archive');
	const remove = isOpen('delete');

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

	/** Opens New team, at the top level or under `parentId`. */
	export function openNew(parentId?: string) {
		newUnder = parentId;
		current = 'new';
	}

	export function open(action: TeamAction, team: TeamRow) {
		target = team;
		if (action === 'add-team') return openNew(team.id);
		if (action === 'lead') void loadCandidates(team.id);
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
<DeleteTeamDialog
	bind:open={remove.get, remove.set}
	team={orgTeam}
	{snapshot}
	{session}
	ondone={async (r) => {
		if ('stale' in r) return reload();
		onremoved({ action: 'delete', teamId: target!.id, name: r.name });
	}}
/>
<ArchiveTeamDialog
	bind:open={archive.get, archive.set}
	team={target}
	{organizationId}
	ondone={(r) => onremoved({ action: 'archive', teamId: target!.id, name: r.name })}
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
