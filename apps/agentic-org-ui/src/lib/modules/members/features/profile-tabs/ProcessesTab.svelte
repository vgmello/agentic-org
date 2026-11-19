<!--
  @component
  Profile › Processes (processes-ux §2, "What this agent follows"): the effective set for its current work, each with
  the level it comes from, its revision and how it relates to the level above. With no current work, the
  organization's set and the spec's sentence.
-->
<script lang="ts">
	import Loaded from '$lib/components/Loaded.svelte';
	import { membersApi } from '../../api';
	import ProcessRows from '../../components/ProcessRows.svelte';
	import { profileContext } from '../profile/context.svelte';
	import { tabResource } from './reload.svelte';

	const ctx = profileContext();
	const processes = tabResource((o, m) => membersApi.processes(o, m));
</script>

<Loaded
	resource={processes}
	label="processes"
	error="Couldn't load the processes {ctx.profile.name} follows"
	isEmpty={(view) => view.rows.length === 0}
	emptyTitle="No processes yet"
	emptyDescription="Write down how your agents work. Start with how changes get reviewed and merged."
>
	{#snippet children(view)}
		<p class="px-4 pt-6 pb-2 text-sm/6 text-gray-400 min-[640px]:px-6 min-[1024px]:px-8">
			{view.hasCurrentWork
				? `What ${ctx.profile.name} follows for its current work, and where each process comes from.`
				: "No current work. Showing the organization's processes. Team and project processes apply when it starts a task."}
		</p>
		<ProcessRows rows={view.rows} />
	{/snippet}
</Loaded>
