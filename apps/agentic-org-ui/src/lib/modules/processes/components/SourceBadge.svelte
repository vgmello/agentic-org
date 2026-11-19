<!--
  @component
  Where a document comes from, as an icon and words: "Organization rev 3", "Team Web rev 2" (PROC-9, PROC-18).
  The words wrap whole; the icon stays with the first word.
-->
<script lang="ts">
	import { sourceText } from '../present';
	import type { ProcessDocument } from '../types';
	import Glyph from './Glyph.svelte';

	let { document: doc }: { document: ProcessDocument } = $props();
</script>

<span
	class="inline-flex max-w-full min-w-0 items-start gap-x-1 rounded-md bg-surface-raised px-2 py-0.5 text-xs/5 text-fg-secondary ring-1 ring-line ring-inset"
	data-testid="level-badge"
>
	<Glyph name={doc.level} class="mt-0.5 text-fg-muted" />
	<span class="min-w-0 [overflow-wrap:anywhere]"
		>{sourceText(doc)} <span class="whitespace-nowrap">rev {doc.revision}</span></span
	>
</span>
