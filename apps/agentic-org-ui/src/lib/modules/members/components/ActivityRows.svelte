<!--
  @component
  Activity rows: actor, what happened and a `<time datetime>` (app-shell-ux §6a feed rows), newest first.
-->
<script lang="ts">
	import { whenText } from '../present';
	import type { ActivityEntry } from '../types';

	let { rows }: { rows: ActivityEntry[] } = $props();
</script>

<ul role="list" class="divide-y divide-white/5">
	{#each rows as row (row.id)}
		<li class="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
			<p class="min-w-0 flex-1 basis-64 text-sm/6 text-gray-300 [overflow-wrap:anywhere]">
				<span class="font-semibold text-white">{row.actor}</span>
				{row.text}
			</p>
			<p class="text-xs/5 text-gray-400"><span class="sr-only">{row.kind}, </span><span aria-hidden="true">{row.kind} · </span><time datetime={row.at}>{whenText(row.at)}</time></p>
		</li>
	{/each}
</ul>
