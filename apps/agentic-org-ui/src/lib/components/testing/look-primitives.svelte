<script lang="ts">
	import Callout from '$lib/components/Callout.svelte';
	import DividedRow from '$lib/components/DividedRow.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
</script>

<section aria-label="Primitive contracts" class="fixed inset-x-4 top-20 z-[100] space-y-3 rounded-lg bg-gray-800 p-4">
	<Callout data-testid="quiet-contract">Static information</Callout>
	<Callout role="status" data-testid="status-contract">Saved progress</Callout>
	<Callout role="alert" data-testid="alert-contract">Failed update</Callout>
	<Callout data-testid="icon-callout">
		{#snippet icon()}<svg role="img" aria-label="Callout ornament" viewBox="0 0 16 16" class="size-4" fill="currentColor"><circle cx="8" cy="8" r="4" /></svg>{/snippet}
		Informative context
	</Callout>
	<StatusPill label="Status unknown" data-testid="word-only" />
	<StatusPill label="Paused" data-testid="word-and-icon">
		{#snippet icon()}<svg role="img" aria-label="Status ornament" viewBox="0 0 16 16" class="size-4" fill="currentColor"><circle cx="8" cy="8" r="4" /></svg>{/snippet}
	</StatusPill>
	<div class="flex w-32 max-w-full flex-wrap gap-2" data-testid="wrapping-pills">
		<StatusPill label="On hold" />
		<StatusPill label="Ready now" />
	</div>
	<div class="w-40 max-w-full"><StatusPill label={'Status' + 'x'.repeat(64)} data-testid="long-label">
		{#snippet icon()}<svg viewBox="0 0 16 16" class="size-4" fill="currentColor"><circle cx="8" cy="8" r="4" /></svg>{/snippet}
	</StatusPill></div>
	<div class="w-40 max-w-full"><Callout data-testid="long-callout">{'Information' + 'x'.repeat(64)}</Callout></div>
	<ol start="4" aria-label="Ordered facts"><DividedRow value={7} lang="pt" title="Native row" data-testid="native-row">Seventh fact</DividedRow></ol>
</section>
