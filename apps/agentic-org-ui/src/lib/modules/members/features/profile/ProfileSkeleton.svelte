<!--
  @component
  The profile's first load (app-shell-ux §6a): shaped like what arrives, the header (avatar, name, facts, presence),
  the tab strip and two sections of term and value rows, so nothing jumps when it does. A polite status named
  "Loading profile", "Still loading profile" after two seconds. Reduced motion stops the pulse.
-->
<script lang="ts">
	let slow = $state(false);
	$effect(() => {
		const timer = setTimeout(() => (slow = true), 2000);
		return () => clearTimeout(timer);
	});

	const bar = 'block animate-pulse rounded bg-surface-overlay motion-reduce:animate-none';
	const quiet = 'block animate-pulse rounded bg-surface-raised motion-reduce:animate-none';
	const gutters = 'px-4 min-[640px]:px-6 min-[1024px]:px-8';
	const tabs = [16, 10, 24, 14, 10];
	const sections = [2, 4];
</script>

<div role="status" aria-busy="true">
	<span class="sr-only">{slow ? 'Still loading profile' : 'Loading profile'}</span>
	<div aria-hidden="true">
		<div class="profile-masthead border-b border-line py-6 {gutters}">
			<span class="{quiet} mb-3 h-4 w-20"></span>
			<div class="flex flex-col gap-3 min-[640px]:flex-row min-[640px]:gap-x-4">
				<span
					class="size-10 shrink-0 animate-pulse rounded-full bg-surface-overlay motion-reduce:animate-none"
				></span>
				<span class="min-w-0 flex-1">
					<span class="{bar} h-5 w-48 max-w-full"></span>
					<span class="{quiet} mt-3 h-4 w-40 max-w-full"></span>
					<span class="{quiet} mt-3 h-3 w-72 max-w-full"></span>
					<span class="{quiet} mt-4 h-6 w-36 max-w-full rounded-full"></span>
				</span>
			</div>
			{#if slow}<p class="mt-4 text-xs/5 text-fg-muted">Still loading…</p>{/if}
		</div>
		<div class="flex gap-x-6 overflow-hidden border-b border-line py-4 {gutters}">
			{#each tabs as width, i (i)}
				<span class="{quiet} h-4 shrink-0" style:width="{width * 0.25}rem"></span>
			{/each}
		</div>
		{#each sections as rows, s (s)}
			<div class="py-6 {s ? 'border-t border-line-subtle' : ''}">
				<div class={gutters}><span class="{bar} h-5 w-32"></span></div>
				<ul class="mt-4 divide-y divide-line-subtle">
					{#each { length: rows }, r (r)}
						<li class="grid gap-2 py-4 min-[640px]:grid-cols-3 min-[640px]:gap-4 {gutters}">
							<span class="{quiet} h-4 w-24"></span>
							<span class="{quiet} h-4 min-[640px]:col-span-2" style:width="{40 + ((r * 17) % 35)}%"
							></span>
						</li>
					{/each}
				</ul>
			</div>
		{/each}
	</div>
</div>

<style>
	.profile-masthead {
		background: var(--exec-well);
	}
</style>
