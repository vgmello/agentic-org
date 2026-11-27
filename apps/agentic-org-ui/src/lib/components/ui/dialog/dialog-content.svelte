<script lang="ts">
	import { Dialog as DialogPrimitive } from 'bits-ui';
	import XIcon from '@lucide/svelte/icons/x';
	import { returnFocus } from '$lib/components/focus';
	import { Button } from '$lib/components/ui/button/index.js';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
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
		onOpenAutoFocus,
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

	let capturedOpener: HTMLElement | null = null;

	const openFocus = (e: Event) => {
		const active = typeof document !== 'undefined' ? document.activeElement : null;
		capturedOpener =
			active instanceof HTMLElement && active !== document.body && !ref?.contains(active)
				? active
				: null;
		onOpenAutoFocus?.(e);
	};

	const closeFocus = (e: Event) => {
		onCloseAutoFocus?.(e);
		const captured = capturedOpener;
		capturedOpener = null;
		if (e.defaultPrevented) return;
		const target = opener !== undefined ? opener : captured;
		const targetUsable = Boolean(
			target?.isConnected && !target.hasAttribute('disabled') && !target.closest('[inert]')
		);
		if (!targetUsable && fallback === undefined) return;
		e.preventDefault();
		void returnFocus(target, { within: ref, fallback });
	};
</script>

<DialogPortal {...portalProps}>
	<DialogOverlay class={overlayClass} />
	<DialogPrimitive.Content
		bind:ref
		data-slot="dialog-content"
		onOpenAutoFocus={openFocus}
		onCloseAutoFocus={closeFocus}
		class={cn(
			'bg-popover text-popover-foreground data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 data-closed:zoom-out-95 data-open:zoom-in-95 ring-foreground/10 grid max-w-[calc(100%_-_2rem)] gap-6 rounded-xl p-6 text-sm ring-1 duration-100 sm:max-w-md fixed top-1/2 left-1/2 z-50 w-full -translate-x-1/2 -translate-y-1/2 outline-none',
			className
		)}
		{...restProps}
	>
		{@render children?.()}
		{#if showCloseButton}
			<ModalTooltip label="Close" class="absolute top-3 right-3">
				<DialogPrimitive.Close data-slot="dialog-close">
					{#snippet child({ props: closeProps })}
						<Button
							variant="ghost"
							class="size-9 [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11"
							size="icon-sm"
							aria-label="Close"
							{...closeProps}
						>
							<XIcon aria-hidden="true" />
						</Button>
					{/snippet}
				</DialogPrimitive.Close>
			</ModalTooltip>
		{/if}
	</DialogPrimitive.Content>
</DialogPortal>
