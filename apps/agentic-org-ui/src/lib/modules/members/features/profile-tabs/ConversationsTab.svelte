<!--
  @component
  Profile › Conversations: the channels, direct conversations and threads this member takes part in, newest first.
  Each row opens it in Chat.
-->
<script lang="ts">
	import Loaded from '$lib/components/Loaded.svelte';
	import { membersApi } from '../../api';
	import ConversationRows from '../../components/ConversationRows.svelte';
	import { profileContext } from '../profile/context.svelte';
	import { tabResource } from './reload.svelte';

	const ctx = profileContext();
	const conversations = tabResource((o, m) => membersApi.conversations(o, m));
</script>

<Loaded
	resource={conversations}
	label="conversations"
	error="Couldn't load {ctx.profile.name}'s conversations"
	isEmpty={(rows) => rows.length === 0}
	emptyTitle="No conversations yet"
	emptyDescription="Channels, direct conversations and threads {ctx.profile
		.name} takes part in show here."
>
	{#snippet children(rows)}
		<ConversationRows {rows} />
	{/snippet}
</Loaded>
