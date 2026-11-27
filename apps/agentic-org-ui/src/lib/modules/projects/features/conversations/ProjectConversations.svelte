<!--
  @component
  A project's Conversations tab (work-items-ux §3, PRJ-2): the threads and channels linked to the project that the
  viewer may read, latest first; each opens Chat. Empty: "No conversations yet".
-->
<script lang="ts">
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { ConversationRows } from '$lib/modules/members/ui';
	import { projectsApi } from '../../api';
	import { projectFrame } from '../frame/context';

	const frame = projectFrame();
	const conversations = new Resource(() =>
		projectsApi.conversations(frame.organizationId, frame.projectId)
	);
</script>

<Loaded
	resource={conversations}
	label="the project's conversations"
	error="Couldn't load the project's conversations"
	errorLevel={4}
>
	{#snippet children(rows)}
		{#if rows.length === 0}
			<EmptyState
				level={4}
				title="No conversations yet"
				description="Threads about {frame.project
					.name}'s tasks and bugs, and channels linked to it, show here once someone starts one."
			/>
		{:else}
			<ConversationRows {rows} />
		{/if}
	{/snippet}
</Loaded>
