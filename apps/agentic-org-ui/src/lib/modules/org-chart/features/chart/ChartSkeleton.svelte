<!--
  @component
  The org chart's first load (offices-and-teams-ux §7.10, app-shell-ux §6a): the owner bar's shape, then indented
  nodes, as one polite status named "Loading the org chart", "Still loading" after two seconds. Reduced motion stops
  the pulse.
-->
<script lang="ts">
	let slow = $state(false);
	$effect(() => {
		const timer = setTimeout(() => (slow = true), 2000);
		return () => clearTimeout(timer);
	});
	const shape = 'animate-pulse rounded bg-white/10 motion-reduce:animate-none';
	const nodes = [1, 2, 2, 3, 1, 2];
</script>

<div role="status" aria-busy="true" class="px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
	<span class="sr-only">{slow ? 'Still loading the org chart' : 'Loading the org chart'}</span>
	<div aria-hidden="true">
		{#if slow}<p class="mb-4 text-xs/5 text-gray-400">Still loading…</p>{/if}
		<div class="flex items-center gap-x-4 rounded-lg border border-white/10 bg-black/10 p-4">
			<span class="size-12 shrink-0 {shape} rounded-full"></span>
			<span class="min-w-0 flex-1">
				<span class="block h-4 w-1/3 {shape}"></span>
				<span class="mt-3 block h-3 w-1/4 {shape} bg-white/5"></span>
			</span>
		</div>
		<span class="mt-8 block h-4 w-40 {shape}"></span>
		<ul class="mt-4">
			{#each nodes as level, index (index)}
				<li class="flex items-center gap-x-3 py-3" style:padding-left="{(level - 1) * 1.5}rem">
					<span class="size-9 shrink-0 {shape} rounded-full"></span>
					<span class="min-w-0 flex-1">
						<span class="block h-4 {shape}" style:width="{30 + ((index * 17) % 30)}%"></span>
						<span class="mt-2 block h-3 w-1/5 {shape} bg-white/5"></span>
					</span>
				</li>
			{/each}
		</ul>
	</div>
</div>
