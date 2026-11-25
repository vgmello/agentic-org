<!--
  @component
  Summary metric cell rendered inside a `<dl>` grid.
    <dl class="grid grid-cols-1 gap-px bg-surface-raised min-[640px]:grid-cols-2 min-[1280px]:grid-cols-4">
      <StatTile label="Open work" value="12" detail="3 need you" href="/work?needs=me" />
    </dl>
  `value` is preformatted text (counts, spend); pass "Unknown" rather than guessing.
  When `href` is supplied, the label link stretches over the cell.
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
	class="relative flex min-w-0 flex-wrap content-start items-baseline justify-between gap-x-4 gap-y-2 bg-surface px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8"
>
	<dt class="min-w-0 text-sm/6 font-medium wrap-break-word text-fg-muted">
		{#if href}
			<a
				href={resolve(href as '/')}
				class="rounded-sm transition-colors duration-150 hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus motion-reduce:transition-none"
				><span class="absolute inset-0" aria-hidden="true"></span>{label}</a
			>
		{:else}{label}{/if}
	</dt>
	{#if detail}
		<dd class="min-w-0 text-xs/5 wrap-break-word text-fg-muted">{detail}</dd>
	{/if}
	<dd
		class="w-full min-w-0 flex-none text-3xl/9 font-semibold tracking-tight wrap-break-word text-fg tabular-nums"
	>
		{value}
	</dd>
</div>
