<!--
  @component
  First-load placeholder rows shaped like divided list rows (app-shell-ux §6a). It is a polite status
  region named "Loading {label}"; after two seconds it says "Still loading". Reduced motion stops the pulse.
  Match the eventual layout: `lines={2}` for title + meta rows, `avatar` when rows lead with an avatar.
  Usage: {#if loading}<SkeletonRows label="members" rows={6} avatar />{/if}
-->
<script lang="ts">
	let {
		label,
		rows = 5,
		lines = 2,
		avatar = false
	}: { label: string; rows?: number; lines?: 1 | 2; avatar?: boolean } = $props();

	let slow = $state(false);
	$effect(() => {
		const timer = setTimeout(() => (slow = true), 2000);
		return () => clearTimeout(timer);
	});
</script>

<div role="status" aria-busy="true">
	<span class="sr-only">{slow ? `Still loading ${label}` : `Loading ${label}`}</span>
	{#if slow}<p
			aria-hidden="true"
			class="px-4 pt-4 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8"
		>
			Still loading…
		</p>{/if}
	<ul aria-hidden="true" class="divide-y divide-line-subtle">
		{#each { length: rows }, index (index)}
			<li class="flex items-center gap-x-4 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
				{#if avatar}<span
						class="size-9 shrink-0 animate-pulse rounded-full bg-surface-overlay motion-reduce:animate-none"
					></span>{/if}
				<span class="min-w-0 flex-1">
					<span
						class="block h-4 animate-pulse rounded bg-surface-overlay motion-reduce:animate-none"
						style:width="{40 + ((index * 17) % 35)}%"
					></span>
					{#if lines === 2}<span
							class="mt-3 block h-3 w-1/4 animate-pulse rounded bg-surface-raised motion-reduce:animate-none"
						></span>{/if}
				</span>
			</li>
		{/each}
	</ul>
</div>
