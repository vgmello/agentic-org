<script lang="ts">
	import { statusInfo } from '$lib/offices/model';
	import type { OfficeStatus } from '$lib/proto';

	let { status }: { status: OfficeStatus | undefined } = $props();
	const info = $derived(statusInfo(status));
	// The word is always shown: color alone never carries the status.
	const dot = { ok: 'bg-green-400', info: 'bg-sky-400', muted: 'bg-gray-500', warn: 'bg-amber-400', bad: 'bg-red-400' } as const;
</script>

<span class="inline-flex items-center gap-x-1.5 text-xs/5 text-gray-200">
	<span class="size-2 shrink-0 rounded-full {dot[info.tone]}" aria-hidden="true"></span>
	{info.label}
</span>
