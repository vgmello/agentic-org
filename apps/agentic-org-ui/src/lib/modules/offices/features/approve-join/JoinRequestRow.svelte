<!--
  @component
  The "Wants to join" row (§6.1, OFC-5): highlighted, above the offices, with Review and approve. Amber means the owner is needed.
-->
<script lang="ts">
	import Bell from '@lucide/svelte/icons/bell';
	import { Button } from '$lib/components/ui/button';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { tone } from '$lib/components/pills';
	import { agoText } from '../../time';
	import type { JoinRequest } from '../../types';

	let {
		request,
		now,
		onreview
	}: { request: JoinRequest; now: number; onreview: (button: HTMLElement) => void } = $props();
</script>

<li
	class="flex flex-wrap items-center justify-between gap-x-4 gap-y-3 border-b border-line-subtle bg-warning/5 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
	data-testid="join-request"
	data-request={request.id}
>
	<div class="min-w-0 flex-1 basis-56">
		<p
			class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]"
		>
			{request.officeName} wants to join
			<StatusPill label="Wants to join" class={tone.owner}
				>{#snippet icon()}<Bell
						class="size-3.5 text-warning"
						aria-hidden="true"
					/>{/snippet}</StatusPill
			>
		</p>
		<p class="mt-1 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
			from {request.fromAddress} · {agoText(request.requestedAt, now)}
		</p>
	</div>
	<Button
		variant="outline"
		class="max-[639px]:min-h-11"
		aria-label="Review and approve {request.officeName}"
		onclick={(e) => onreview(e.currentTarget)}>Review and approve</Button
	>
</li>
