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

<div class="relative flex flex-col justify-between gap-2 bg-surface px-4 py-5 min-[640px]:px-6">
	<div class="flex items-baseline justify-between gap-3">
		<dt class="text-sm leading-5 font-medium text-fg-muted">
			{#if href}
				<a
					href={resolve(href as '/')}
					class="rounded-sm transition-colors hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
				>
					<span class="absolute inset-0" aria-hidden="true"></span>{label}
				</a>
			{:else}
				{label}
			{/if}
		</dt>
		{#if detail}
			<dd class="shrink-0 text-xs leading-5 text-fg-muted">{detail}</dd>
		{/if}
	</div>
	<dd class="text-3xl leading-9 font-semibold tracking-tight text-fg tabular-nums">{value}</dd>
</div>
