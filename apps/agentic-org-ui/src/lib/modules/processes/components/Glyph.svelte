<!--
  @component
  A solid 16 status or meta mark beside its word (never alone: processes-ux PROC-18). Decorative.
-->
<script lang="ts">
	import { glyphs, type GlyphName } from './glyphs';

	let { name, class: className = 'text-fg-muted' }: { name: GlyphName; class?: string } = $props();
</script>

<svg
	viewBox="0 0 16 16"
	fill="currentColor"
	aria-hidden="true"
	focusable="false"
	class="size-4 shrink-0 {className}"
>
	<path fill-rule="evenodd" clip-rule="evenodd" d={glyphs[name]} />
</svg>
