<script lang="ts">
	import type { ChatState } from './types';
	import type { ChatContext } from './engine';
	import { messageRows } from './row-data';
	import MessageRow from './components/MessageRow.svelte';
	let { state, ctx }: { state: ChatState; ctx: ChatContext } = $props();
	const rows = $derived(messageRows(state, ctx));
</script>
{#each state.messages as message (message.id)}
	<MessageRow {message} {ctx} rowData={rows.forMessage(message)} now={ctx.now} />
{/each}
