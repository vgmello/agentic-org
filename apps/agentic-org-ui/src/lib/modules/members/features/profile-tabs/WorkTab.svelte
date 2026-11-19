<!--
  @component
  Profile › Work (offices-and-teams-ux §7.2 rows): the work assigned to this member, reference and title, project and
  priority, one work-state pill. The assignee is always them, so it isn't repeated.
-->
<script lang="ts">
	import Loaded from '$lib/components/Loaded.svelte';
	import { Button } from '$lib/components/ui/button';
	import { mobileButton } from '$lib/components/classes';
	import { membersApi } from '../../api';
	import WorkRows from '../../components/WorkRows.svelte';
	import { profileContext } from '../profile/context.svelte';
	import { tabResource } from './reload.svelte';

	const ctx = profileContext();
	const work = tabResource((o, m) => membersApi.work(o, m));
</script>

<Loaded
	resource={work}
	label="work"
	error="Couldn't load {ctx.profile.name}'s work"
	isEmpty={(rows) => rows.length === 0}
	emptyTitle="No work assigned"
	emptyDescription="Work items assigned to {ctx.profile.name} show here."
>
	{#snippet emptyAction()}
		{#if ctx.profile.status !== 'inactive'}
			<Button
				variant="outline"
				class={mobileButton}
				onclick={(e) => ctx.act('assign', e.currentTarget)}>Assign task</Button
			>
		{/if}
	{/snippet}
	{#snippet children(rows)}
		<WorkRows {rows} hideAssignee />
	{/snippet}
</Loaded>
