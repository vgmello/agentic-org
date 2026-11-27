<script lang="ts">
	import { Tooltip as TooltipPrimitive } from 'bits-ui';
	import { cn } from '$lib/utils.js';

	let {
		ref = $bindable(null),
		class: className,
		sideOffset = 6,
		role = 'tooltip',
		children,
		...restProps
	}: TooltipPrimitive.ContentProps = $props();
</script>

<TooltipPrimitive.Portal>
	<TooltipPrimitive.Content
		bind:ref
		data-slot="tooltip-content"
		{sideOffset}
		{role}
		class={cn(
			'z-50 max-w-xs rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line motion-reduce:animate-none motion-reduce:transition-none',
			className
		)}
		{...restProps}
	>
		{@render children?.()}
	</TooltipPrimitive.Content>
</TooltipPrimitive.Portal>
