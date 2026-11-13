<script lang="ts">
	import { Dialog as DialogPrimitive } from "bits-ui";
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from "$lib/components/ui/button/index.js";
	import { returnFocus } from "$lib/a11y/focus-return.js";
	import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";
	import DialogOverlay from "./dialog-overlay.svelte";
	import DialogPortal from "./dialog-portal.svelte";
	import type { Snippet } from "svelte";
	import type { ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		class: className,
		portalProps,
		children,
		showCloseButton = true,
		opener,
		fallback,
		onCloseAutoFocus,
		...restProps
	}: WithoutChildrenOrChild<DialogPrimitive.ContentProps> & {
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof DialogPortal>>;
		children: Snippet;
		showCloseButton?: boolean;
		/** What opened the dialog (`openerFrom(event)`): focus goes back to it on close, even where a click does not focus a button (WebKit). Without it nothing changes. */
		opener?: HTMLElement | null;
		/** Where focus goes when `opener` is gone or cannot take it. */
		fallback?: () => HTMLElement | null;
	} = $props();

	const closeFocus = (e: Event) => {
		onCloseAutoFocus?.(e);
		if (!opener || e.defaultPrevented) return;
		e.preventDefault();
		void returnFocus(opener, { within: ref, fallback });
	};
</script>

<DialogPortal {...portalProps}>
	<DialogOverlay />
	<DialogPrimitive.Content
		bind:ref
		data-slot="dialog-content"
		onCloseAutoFocus={closeFocus}
		class={cn(
			"bg-popover text-popover-foreground data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 data-closed:zoom-out-95 data-open:zoom-in-95 ring-foreground/10 grid max-w-[calc(100%_-_2rem)] gap-6 rounded-xl p-6 text-sm ring-1 duration-100 sm:max-w-md fixed top-1/2 left-1/2 z-50 w-full -translate-x-1/2 -translate-y-1/2 outline-none",
			className
		)}
		{...restProps}
	>
		{@render children?.()}
		{#if showCloseButton}
			<DialogPrimitive.Close data-slot="dialog-close">
				{#snippet child({ props })}
					<Button variant="ghost" class="absolute top-4 right-4" size="icon-sm" {...props}>
						<XIcon  />
						<span class="sr-only">Close</span>
					</Button>
				{/snippet}
			</DialogPrimitive.Close>
		{/if}
	</DialogPrimitive.Content>
</DialogPortal>
