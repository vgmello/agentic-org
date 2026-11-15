<script lang="ts">
	import Dialog from './dialog.svelte';
	import DialogTitle from './dialog-title.svelte';
	let { text, legacy = false, name }: { text?: string; legacy?: boolean; name?: string } = $props();
</script>

<Dialog>
	{#if legacy}<DialogTitle>Edit {name}</DialogTitle>{:else}<DialogTitle {text}>Fallback title</DialogTitle>{/if}
</Dialog>
