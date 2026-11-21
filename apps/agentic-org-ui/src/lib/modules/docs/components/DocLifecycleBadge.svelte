<!--
  @component
  Lifecycle pill for a project document: Draft, Current, or Deprecated, always icon plus word (DEC-2).
-->
<script lang="ts">
	import CircleDashedIcon from '@lucide/svelte/icons/circle-dashed';
	import CheckCircle2Icon from '@lucide/svelte/icons/check-circle-2';
	import ArchiveIcon from '@lucide/svelte/icons/archive';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { tone } from '$lib/components/pills';
	import { LIFECYCLE_TONE, LIFECYCLE_WORD } from '../present';
	import type { DocumentLifecycle } from '../types';

	let { status }: { status: DocumentLifecycle } = $props();
</script>

<StatusPill label={LIFECYCLE_WORD[status]} class={tone[LIFECYCLE_TONE[status]]}>
	{#snippet icon()}
		{#if status === 'draft'}
			<CircleDashedIcon class="size-3.5" aria-hidden="true" />
		{:else if status === 'current'}
			<CheckCircle2Icon class="size-3.5" aria-hidden="true" />
		{:else}
			<ArchiveIcon class="size-3.5" aria-hidden="true" />
		{/if}
	{/snippet}
</StatusPill>
