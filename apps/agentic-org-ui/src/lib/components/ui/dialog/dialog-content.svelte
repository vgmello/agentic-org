<script lang="ts">
	import { Dialog as DialogPrimitive } from 'bits-ui';
	import XIcon from '@lucide/svelte/icons/x';
	import { returnFocus } from '$lib/components/focus';
	import { Button } from '$lib/components/ui/button/index.js';
	import { cn, type WithoutChildrenOrChild } from '$lib/utils.js';
	import DialogOverlay from './dialog-overlay.svelte';
	import DialogPortal from './dialog-portal.svelte';
	import type { Snippet } from 'svelte';
	import type { ComponentProps } from 'svelte';

	let {
		ref = $bindable(null),
		class: className,
		overlayClass,
		portalProps,
		children,
		showCloseButton = true,
		opener,
		fallback,
		onCloseAutoFocus,
		...restProps
	}: WithoutChildrenOrChild<DialogPrimitive.ContentProps> & {
		overlayClass?: string;
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof DialogPortal>>;
		children: Snippet;
		showCloseButton?: boolean;
		/** The control that opened the dialog: gets focus back on close even when opened by pointer in WebKit. */
		opener?: HTMLElement | null;
		/** Where focus goes if `opener` is gone or unusable when the dialog closes. */
		fallback?: () => HTMLElement | null;
	} = $props();

	const closeFocus = (e: Event) => {
		onCloseAutoFocus?.(e);
		if (e.defaultPrevented || opener === undefined) return;
		e.preventDefault();
		void returnFocus(opener, { within: ref, fallback });
	};
</script>

<DialogPortal {...portalProps}>
	<DialogOverlay class={overlayClass} />
	<DialogPrimitive.Content
		bind:ref
		data-slot="dialog-content"
		onCloseAutoFocus={closeFocus}
		class={cn(
			'bg-popover text-popover-foreground data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 data-closed:zoom-out-95 data-open:zoom-in-95 ring-foreground/10 grid max-w-[calc(100%_-_2rem)] gap-6 rounded-xl p-6 text-sm ring-1 duration-100 sm:max-w-md fixed top-1/2 left-1/2 z-50 w-full -translate-x-1/2 -translate-y-1/2 outline-none',
			className
		)}
		{...restProps}
	>
		{@render children?.()}
		{#if showCloseButton}
			<!-- Avoid Bits UI Tooltip inside DialogContent: PopperLayer mounts a FocusScope that pauses the modal focus trap. -->
			<span class="absolute top-3 right-3 inline-flex">
				<DialogPrimitive.Close data-slot="dialog-close">
					{#snippet child({ props: closeProps })}
						<Button
							variant="ghost"
							class="peer size-9 [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11"
							size="icon-sm"
							aria-label="Close"
							{...closeProps}
						>
							<XIcon aria-hidden="true" />
						</Button>
					{/snippet}
				</DialogPrimitive.Close>
				<span
					role="tooltip"
					data-tooltip-content=""
					class="pointer-events-none invisible absolute top-full right-0 z-[60] mt-1.5 whitespace-nowrap rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg opacity-0 shadow-md ring-1 ring-line transition-opacity delay-150 peer-hover:visible peer-hover:opacity-100 peer-focus-visible:visible peer-focus-visible:opacity-100 motion-reduce:transition-none"
					>Close</span
				>
			</span>
		{/if}
	</DialogPrimitive.Content>
</DialogPortal>
