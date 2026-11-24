<!--
  @component
  What offices have queued for HQ and where it stands (§6.5, OFC-9): Pending or Blocked, in words. A blocked one can be tried again or
  discarded. This is not an unsaved browser draft.
-->
<script lang="ts">
	import DividedRow from '$lib/components/DividedRow.svelte';
	import WordPill from '../status/WordPill.svelte';
	import { Button } from '$lib/components/ui/button';
	import { agoText } from '../../time';
	import type { OutboxItem } from '../../types';

	let {
		items,
		now,
		bleed = '',
		gutters = '',
		onretry,
		ondiscard,
		retrying
	}: {
		items: OutboxItem[];
		now: number;
		bleed?: string;
		gutters?: string;
		onretry: (item: OutboxItem, button: HTMLElement) => void;
		ondiscard: (item: OutboxItem, button: HTMLElement) => void;
		retrying?: string | undefined;
	} = $props();
</script>

<section aria-labelledby="outbox-title" class="flex flex-col gap-3" data-testid="outbox">
	<h3 id="outbox-title" class="text-xs/6 font-semibold text-fg-muted">
		Waiting on offices · {items.length}
	</h3>
	<ul class="{bleed} -mx-4 sm:-mx-6 lg:-mx-8" aria-label="Waiting on offices">
		{#each items as item (item.id)}
			<DividedRow
				class="{gutters} flex flex-wrap items-center justify-between gap-x-4 gap-y-3"
				data-outbox={item.id}
			>
				<div class="min-w-0 flex-1 basis-56">
					<p
						class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]"
					>
						{item.description}
						<WordPill
							label={item.state === 'blocked' ? 'Blocked' : 'Pending'}
							class={item.state === 'blocked' ? 't-amber' : ''}
						/>
					</p>
					<p class="mt-1 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
						{item.officeName} · {agoText(item.since, now)}{item.reason ? ` · ${item.reason}` : ''}
					</p>
				</div>
				{#if item.state === 'blocked'}
					<div class="flex flex-wrap gap-2">
						<Button
							variant="outline"
							class="max-[639px]:min-h-11"
							aria-busy={retrying === item.id}
							aria-disabled={retrying === item.id}
							aria-label="Retry: {item.description}"
							onclick={(e) => retrying !== item.id && onretry(item, e.currentTarget)}
							>{retrying === item.id ? 'Retrying…' : 'Retry'}</Button
						>
						<Button
							variant="outline"
							class="max-[639px]:min-h-11"
							aria-label="Discard: {item.description}"
							onclick={(e) => ondiscard(item, e.currentTarget)}>Discard</Button
						>
					</div>
				{/if}
			</DividedRow>
		{/each}
	</ul>
</section>
