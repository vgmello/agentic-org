<!--
  @component
  A team's Conversations tab (§7.2): the channels, direct conversations and threads its members take part in.
  Each row opens Chat.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { ConversationRows } from '$lib/modules/members';
	import { teamsApi } from '../../api';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { teamPageContext } from './context';

	const team = teamPageContext();

	const conversations = new Resource(() =>
		teamsApi.conversations(team.organizationId, team.teamId)
	);
</script>

<Loaded
	errorLevel={4}
	resource={conversations}
	label="the team's conversations"
	error="Couldn't load the team's conversations"
>
	{#snippet children(rows)}
		{#if rows.length === 0}
			<EmptyState
				level={4}
				title="No conversations yet"
				description="Channels and threads {team.teamName}'s people and agents take part in show here."
			>
				{#snippet action()}
					<Button variant="outline" href={resolve('/chat')} class="max-[639px]:h-11"
						>Open Chat</Button
					>
				{/snippet}
			</EmptyState>
		{:else}
			<ConversationRows {rows} />
		{/if}
	{/snippet}
</Loaded>
