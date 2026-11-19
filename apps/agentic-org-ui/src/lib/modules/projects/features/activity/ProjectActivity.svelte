<!--
  @component
  A project's Activity tab (work-items-ux §3): what happened in it, newest first, each entry with actor, action with
  full references ("Ana assigned T-A42 to Platform · Ana") and time. Empty: "No activity yet".
-->
<script lang="ts">
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { ActivityRows } from '$lib/modules/members';
	import { projectsApi } from '../../api';
	import { projectFrame } from '../frame/context';

	const frame = projectFrame();
	const activity = new Resource(() => projectsApi.activity(frame.organizationId, frame.projectId));
</script>

<Loaded
	resource={activity}
	label="the project's activity"
	error="Couldn't load the project's activity"
	errorLevel={4}
>
	{#snippet children(rows)}
		{#if rows.length === 0}
			<EmptyState
				level={4}
				title="No activity yet"
				description="What happens in {frame.project
					.name} shows here as it happens, newest first: tasks created, assigned and finished, bugs reported, the brief changed."
			/>
		{:else}
			<ActivityRows {rows} />
		{/if}
	{/snippet}
</Loaded>
