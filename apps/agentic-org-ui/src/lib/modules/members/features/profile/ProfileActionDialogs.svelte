<script lang="ts">
	import { toast } from 'svelte-sonner';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import type { sessionFor } from '$lib/modules/organization/session';
	import type { Member, OrgSnapshot } from '$lib/proto';
	import type { MemberProfile, RunNowResult } from '../../types';
	import AssignTaskDialog from '../assign-task/AssignTaskDialog.svelte';
	import RunNowDialog from '../run-now/RunNowDialog.svelte';
	import type { ProfileAction } from './context.svelte';
	import IdentityDialog from './IdentityDialog.svelte';
	import MoveHomeTeamDialog from './MoveHomeTeamDialog.svelte';

	let {
		dialog = $bindable<ProfileAction | undefined>(),
		opening,
		organizationId,
		organizationName,
		profile,
		record,
		snapshot,
		session,
		shownLines,
		onRunRequested,
		onChanged,
		onPause,
		onOffboard,
		onRemove,
		onRestore,
		onRestoreAfterChange
	}: {
		dialog: ProfileAction | undefined;
		opening: number;
		organizationId: string;
		organizationName: string;
		profile: MemberProfile;
		record: Member | undefined;
		snapshot: OrgSnapshot;
		session: ReturnType<typeof sessionFor>;
		shownLines: readonly string[];
		onRunRequested: (result: RunNowResult) => void;
		onChanged: () => Promise<void>;
		onPause: () => Promise<void>;
		onOffboard: () => Promise<void>;
		onRemove: () => Promise<void>;
		onRestore: (event: Event) => void;
		onRestoreAfterChange: (event: Event) => void;
	} = $props();

	const open = (action: ProfileAction) => ({
		get value() {
			return dialog === action;
		},
		set value(next: boolean) {
			dialog = next ? action : dialog === action ? undefined : dialog;
		}
	});
	const runOpen = open('run');
	const assignOpen = open('assign');
	const pauseOpen = open('pause');
	const offboardOpen = open('offboard');
	const removeOpen = open('remove');
	const editOpen = open('edit');
	const moveOpen = open('move');
</script>

{#if profile.kind === 'agent'}
	{#key opening}<RunNowDialog
			bind:open={runOpen.value}
			{organizationId}
			{profile}
			onCloseAutoFocus={onRestore}
			onrefused={() => void onChanged()}
			onstarted={onRunRequested}
		/>{/key}
{/if}
<AssignTaskDialog
	bind:open={assignOpen.value}
	{organizationId}
	{profile}
	onCloseAutoFocus={onRestore}
	onassigned={(task) => {
		toast.success(`Assigned ${task.ref} to ${profile.name}.`);
		void onChanged();
	}}
/>
{#if record}
	<ConfirmDialog
		bind:open={pauseOpen.value}
		title="Pause {profile.name}?"
		description="HQ will record the request. {profile.name} stays in {profile.homeTeam?.name ??
			'its home team'}; new admissions stop when the authoritative control is applied."
		confirmLabel="Pause"
		busyLabel="Pausing…"
		onconfirm={onPause}
		onCloseAutoFocus={onRestoreAfterChange}
	/>
	<ConfirmDialog
		bind:open={offboardOpen.value}
		title="Offboard {profile.name}?"
		description="{profile.name} becomes a former member. This is what changes:"
		confirmLabel="Offboard"
		busyLabel="Offboarding…"
		destructive
		onconfirm={onOffboard}
		onCloseAutoFocus={onRestoreAfterChange}
	>
		{#snippet body()}
			<ul role="list" class="flex list-disc flex-col gap-1 pl-5">
				{#each shownLines as line, i (i)}<li>{line}</li>{/each}
			</ul>
		{/snippet}
	</ConfirmDialog>
	<ConfirmDialog
		bind:open={removeOpen.value}
		title="Remove {profile.name}?"
		description="{profile.name} becomes a former member. This is what changes:"
		confirmLabel="Remove person"
		busyLabel="Removing…"
		destructive
		onconfirm={onRemove}
		onCloseAutoFocus={onRestoreAfterChange}
	>
		{#snippet body()}
			<ul role="list" class="list-disc space-y-1 pl-5">
				{#each shownLines as line, i (i)}<li>{line}</li>{/each}
			</ul>
		{/snippet}
	</ConfirmDialog>
	{#key opening}
		<IdentityDialog
			bind:open={editOpen.value}
			member={record}
			{snapshot}
			{session}
			organization={organizationName}
			onCloseAutoFocus={onRestore}
			ondone={(r) => {
				if (!r.stale) toast.success(`Saved ${r.name}.`);
				void onChanged();
			}}
		/>
		{#if profile.homeTeam}
			<MoveHomeTeamDialog
				bind:open={moveOpen.value}
				{organizationId}
				member={{ id: profile.id, name: profile.name }}
				fromTeamId={profile.homeTeam.id}
				{snapshot}
				onCloseAutoFocus={onRestore}
				ondone={(r) => {
					if ('name' in r) toast.success(`Moved ${r.name} to ${r.to}.`);
					void onChanged();
				}}
			/>
		{/if}
	{/key}
{/if}
