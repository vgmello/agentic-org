<!--
  @component
  One work-status pill per row/card (work-items-ux §2.1, WORK-5):
  To do (gray), In progress (indigo), Blocked on someone else (red), Waiting on the owner (one amber "Needs you"
  pill replacing the ordinary pill), Done (green), Cancelled (neutral outline).
-->
<script lang="ts">
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { tone } from '$lib/components/pills';
	import { STATUS_WORD } from '../present';
	import type { NeedsMe, TaskStatus } from '../types';

	let {
		status,
		needsMe = null
	}: {
		status: TaskStatus;
		needsMe?: NeedsMe | null;
	} = $props();

	const label = $derived(needsMe ? 'Needs you' : STATUS_WORD[status]);

	const pillClass = $derived.by(() => {
		if (needsMe) return tone.owner;
		switch (status) {
			case 'in_progress':
				return 'bg-primary/10 text-brand-text ring-brand/30';
			case 'blocked':
				return tone.bad;
			case 'done':
				return tone.ok;
			case 'cancelled':
				return 'bg-transparent text-fg-muted ring-line-strong';
			default:
				return tone.neutral;
		}
	});
</script>

<StatusPill {label} class={pillClass} data-testid="status-pill" />
