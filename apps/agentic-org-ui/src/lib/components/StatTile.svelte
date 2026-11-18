<!--
  @component
  One figure in a row of stats (Tailwind Plus stats, flat, no shadow). Place tiles inside a `dl`:
    <dl class="grid grid-cols-1 gap-px bg-white/5 min-[640px]:grid-cols-2 min-[1280px]:grid-cols-4">
      <StatTile label="Open work" value="12" detail="3 need you" href="/work?needs=me" />
    </dl>
  `value` is preformatted text (counts, money); show "Unknown" rather than a guessed number. With `href`
  the label becomes the tile's one link.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	let {
		label,
		value,
		detail,
		href
	}: { label: string; value: string; detail?: string; href?: string } = $props();
</script>

<div
	class="relative flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2 bg-gray-800 px-4 py-6 min-[640px]:px-6"
>
	<dt class="text-sm/6 font-medium text-gray-400">
		{#if href}
			<a
				href={resolve(href as '/')}
				class="rounded-sm hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
				><span class="absolute inset-0" aria-hidden="true"></span>{label}</a
			>
		{:else}{label}{/if}
	</dt>
	{#if detail}<dd class="text-xs/5 text-gray-400">{detail}</dd>{/if}
	<dd class="w-full flex-none text-3xl/10 font-semibold tracking-tight text-white">{value}</dd>
</div>
