<!--
  @component
  A project's first load (app-shell-ux §6a), shaped like the page it becomes: the header (name, facts), the tab strip
  and a few rows. A polite status named "Loading the project"; after two seconds it says "Still loading". Reduced
  motion stops the pulse.
-->
<script lang="ts">
	let slow = $state(false);
	const after2s = () => {
		const timer = setTimeout(() => (slow = true), 2000);
		return () => clearTimeout(timer);
	};

	const gutters = 'px-4 min-[640px]:px-6 min-[1024px]:px-8';
	const bar = 'block animate-pulse rounded bg-surface-overlay motion-reduce:animate-none';
	const quiet = 'block animate-pulse rounded bg-surface-raised motion-reduce:animate-none';
</script>

<div role="status" aria-busy="true" data-project-skeleton {@attach after2s}>
	<span class="sr-only">{slow ? 'Still loading the project' : 'Loading the project'}</span>
	<div aria-hidden="true">
		<div class="border-b border-line-subtle pt-2 pb-6 {gutters}">
			<span class="{bar} mt-1.5 h-4 w-40"></span>
			<span class="{quiet} mt-4 h-3 w-96 max-w-full"></span>
			{#if slow}<span class="mt-3 block text-xs/5 text-fg-muted">Still loading…</span>{/if}
		</div>
		<div class="flex flex-wrap gap-x-6 border-b border-line {gutters}">
			{#each [16, 10, 11, 14, 26, 15] as width (width)}
				<span class="flex min-h-11 items-center py-3">
					<span class="{quiet} h-3" style:width="{width * 4}px"></span>
				</span>
			{/each}
		</div>
		<div class="grid grid-cols-1 gap-px min-[640px]:grid-cols-2 min-[1280px]:grid-cols-4">
			{#each [1, 2, 3, 4] as tile (tile)}
				<div class="px-4 py-6 min-[640px]:px-6">
					<span class="{quiet} h-3 w-24"></span>
					<span class="{bar} mt-4 h-6 w-16"></span>
				</div>
			{/each}
		</div>
		<div class="py-6 {gutters}">
			<span class="{bar} h-4 w-20"></span>
			{#each [92, 80, 64] as width (width)}
				<span class="{quiet} mt-4 h-3 max-w-full" style:width="{width}%"></span>
			{/each}
		</div>
	</div>
</div>
