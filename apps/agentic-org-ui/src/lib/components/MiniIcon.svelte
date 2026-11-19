<!--
  @component
  A Heroicons solid 20 ("mini") icon for use inside controls (app-shell-ux §6a). Decorative: hidden from
  assistive technology, so the control around it carries the name.
  Usage: <MiniIcon name="plus" class="size-5 text-gray-400" />
-->
<script lang="ts">
	import { miniIcons, type MiniIconName } from './icons';

	let { name, class: className = 'size-5' }: { name: MiniIconName; class?: string } = $props();
</script>

<svg
	viewBox="0 0 20 20"
	fill="currentColor"
	aria-hidden="true"
	focusable="false"
	class="shrink-0 {className}"
>
	<path fill-rule="evenodd" clip-rule="evenodd" d={miniIcons[name]} />
</svg>
