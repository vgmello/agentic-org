<!--
  @component
  Initials for an agent or a person (app-shell-ux §6a): one round shape for both; the kind is the word
  Agent or Person elsewhere in the row, never the avatar's shape or a robot glyph. Decorative: the name is
  always written beside it. Sizes: sm 24 px (10 px decorative initials), md 36 px, lg 48 px.
  Usage: <Avatar name={member.name} size="md" />
-->
<script lang="ts">
	import { initialsOf } from '$lib/format';

	let {
		name,
		size = 'md',
		class: className = ''
	}: { name: string; size?: 'sm' | 'md' | 'lg'; class?: string } = $props();
	const sizes = { sm: 'size-6 text-[10px]', md: 'size-9 text-xs', lg: 'size-12 text-sm' } as const;

	function tintIndex(label: string): number {
		let h = 0;
		for (let i = 0; i < label.length; i++) h = (h * 31 + label.charCodeAt(i)) | 0;
		return Math.abs(h) % 7;
	}
</script>

<span
	aria-hidden="true"
	data-tint={tintIndex(name)}
	class="ao-avatar inline-flex shrink-0 items-center justify-center rounded-full bg-primary/20 font-semibold text-brand-text-strong {sizes[
		size
	]} {className}">{initialsOf(name)}</span
>
