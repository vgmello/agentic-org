<!--
  @component
  A team page's first load (app-shell-ux §6a): shaped like the page it becomes, so nothing jumps when it arrives —
  the header (name, facts, purpose, actions), the members heading and three rows, then the tab strip. The back
  link above it is real and stays usable. A polite status named "Loading the team"; after two seconds it says
  "Still loading". Reduced motion stops the pulse.
-->
<script lang="ts">
	let slow = $state(false);
	/** After two seconds the label says "Still loading" (§6a). */
	const after2s = () => {
		const timer = setTimeout(() => (slow = true), 2000);
		return () => clearTimeout(timer);
	};

	const gutters = 'px-4 min-[640px]:px-6 min-[1024px]:px-8';
	const bar = 'block animate-pulse rounded bg-surface-overlay motion-reduce:animate-none';
	const quiet = 'block animate-pulse rounded bg-surface-raised motion-reduce:animate-none';
</script>

<div role="status" aria-busy="true" data-team-skeleton {@attach after2s}>
	<span class="sr-only">{slow ? 'Still loading the team' : 'Loading the team'}</span>
	<div aria-hidden="true">
		<div class="border-b border-line-subtle pt-2 pb-6 {gutters}">
			<div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-4">
				<div class="min-w-0 flex-1 basis-80">
					<span class="{bar} mt-1.5 h-4 w-40"></span>
					<span class="{quiet} mt-4 h-3 w-64 max-w-full"></span>
					<span class="{quiet} mt-4 h-3 w-96 max-w-full"></span>
					{#if slow}<span class="mt-3 block text-xs/5 text-fg-muted">Still loading…</span>{/if}
				</div>
				<div class="flex gap-3">
					<span class="{quiet} h-9 w-28 rounded-md max-[639px]:h-11"></span>
					<span class="{quiet} h-9 w-28 rounded-md max-[639px]:h-11"></span>
				</div>
			</div>
		</div>
		<div class="border-b border-line-subtle py-6 {gutters}">
			<span class="{bar} my-1 h-4 w-28"></span>
			<ul class="mt-3 grid grid-cols-1 gap-x-8 min-[1024px]:grid-cols-2 min-[1440px]:grid-cols-3">
				{#each [52, 40, 46] as width (width)}
					<li class="flex items-center gap-x-3 border-t border-line-subtle py-4">
						<span
							class="size-9 shrink-0 animate-pulse rounded-full bg-surface-overlay motion-reduce:animate-none"
						></span>
						<span class="min-w-0 flex-1">
							<span class="{bar} h-4" style:width="{width}%"></span>
							<span class="{quiet} mt-3 h-3 w-1/3"></span>
						</span>
					</li>
				{/each}
			</ul>
		</div>
		<div class="flex flex-wrap gap-x-6 border-b border-line {gutters}">
			{#each [10, 16, 14, 24, 18] as width (width)}
				<span class="flex min-h-11 items-center py-3">
					<span class="{quiet} h-3" style:width="{width * 4}px"></span>
				</span>
			{/each}
		</div>
	</div>
</div>
