<!--
  @component
  The difference between a document and the one it replaces, line by line: removed lines are marked "Removed" and
  added lines "Added", as words as well as colour (processes-ux §2 "the difference from the parent").
-->
<script lang="ts">
	import { diffLines, diffSummary } from '../present';

	let { before, after, label }: { before: string; after: string; label: string } = $props();
	const lines = $derived(diffLines(before, after));
</script>

<figure class="min-w-0" data-testid="diff">
	<figcaption class="text-xs/5 text-fg-muted">{label} · {diffSummary(lines)}</figcaption>
	<ol
		role="list"
		class="mt-2 overflow-hidden rounded-md bg-surface-sunken font-mono text-xs/5 outline-1 -outline-offset-1 outline-line"
	>
		{#each lines as line, i (i)}
			<li
				class="flex gap-x-2 px-3 py-0.5 {line.kind === 'added'
					? 'bg-positive-emphasis/10 text-fg-bright'
					: line.kind === 'removed'
						? 'bg-danger-emphasis/10 text-fg-secondary'
						: 'text-fg-muted'}"
				data-kind={line.kind}
			>
				<span class="w-3 shrink-0 select-none" aria-hidden="true"
					>{line.kind === 'added' ? '+' : line.kind === 'removed' ? '−' : ''}</span
				>
				<span class="sr-only"
					>{line.kind === 'added' ? 'Added: ' : line.kind === 'removed' ? 'Removed: ' : ''}</span
				>
				<span class="min-w-0 whitespace-pre-wrap [overflow-wrap:anywhere]">{line.text || ' '}</span>
			</li>
		{/each}
	</ol>
</figure>
