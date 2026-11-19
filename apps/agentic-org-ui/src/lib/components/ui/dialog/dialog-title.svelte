<script lang="ts">
	import { Dialog as DialogPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		text,
		children,
		...restProps
	}: DialogPrimitive.TitleProps & { text?: string } = $props();
</script>

<DialogPrimitive.Title
	bind:ref
	data-slot="dialog-title"
	class={cn("leading-none font-medium [overflow-wrap:normal]", className)}
	{...restProps}
>
	{#if text !== undefined}{#each text.split(/(\s+)/u) as word, index (index)}{#if /^\s*$/u.test(word)}{word}{:else}<span data-slot="dialog-title-word" class="inline-block [overflow-wrap:anywhere]">{word}</span>{/if}{/each}{:else}{@render children?.()}{/if}
</DialogPrimitive.Title>
