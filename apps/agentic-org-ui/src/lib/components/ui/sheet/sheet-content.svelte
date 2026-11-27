<script lang="ts" module>
	export type Side = 'top' | 'right' | 'bottom' | 'left';
</script>

<script lang="ts">
	import { Dialog as SheetPrimitive } from 'bits-ui';
	import XIcon from '@lucide/svelte/icons/x';
	import { returnFocus } from '$lib/components/focus';
	import { Button } from '$lib/components/ui/button/index.js';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import { cn, type WithoutChildrenOrChild } from '$lib/utils.js';
	import SheetOverlay from './sheet-overlay.svelte';
	import SheetPortal from './sheet-portal.svelte';
	import type { Snippet } from 'svelte';
	import type { ComponentProps } from 'svelte';

	let {
		ref = $bindable(null),
		class: className,
		side = 'right',
		showCloseButton = true,
		portalProps,
		children,
		opener,
		fallback,
		onOpenAutoFocus,
		onCloseAutoFocus,
		...restProps
	}: WithoutChildrenOrChild<SheetPrimitive.ContentProps> & {
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof SheetPortal>>;
		side?: Side;
		showCloseButton?: boolean;
		children: Snippet;
		opener?: HTMLElement | null;
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

<SheetPortal {...portalProps}>
	<SheetOverlay />
	<SheetPrimitive.Content
		bind:ref
		data-slot="sheet-content"
		data-side={side}
		onOpenAutoFocus={openFocus}
		onCloseAutoFocus={closeFocus}
		class={cn(
			'bg-popover text-popover-foreground fixed z-50 flex flex-col gap-4 bg-clip-padding text-sm shadow-lg transition duration-200 ease-in-out data-[side=bottom]:inset-x-0 data-[side=bottom]:bottom-0 data-[side=bottom]:h-auto data-[side=bottom]:border-t data-[side=left]:inset-y-0 data-[side=left]:left-0 data-[side=left]:h-full data-[side=left]:w-3/4 data-[side=left]:border-r data-[side=right]:inset-y-0 data-[side=right]:right-0 data-[side=right]:h-full data-[side=right]:w-3/4 data-[side=right]:border-l data-[side=top]:inset-x-0 data-[side=top]:top-0 data-[side=top]:h-auto data-[side=top]:border-b data-[side=left]:sm:max-w-sm data-[side=right]:sm:max-w-sm data-open:animate-in data-open:fade-in-0 data-[side=bottom]:data-open:slide-in-from-bottom-10 data-[side=left]:data-open:slide-in-from-left-10 data-[side=right]:data-open:slide-in-from-right-10 data-[side=top]:data-open:slide-in-from-top-10 data-closed:animate-out data-closed:fade-out-0 data-[side=bottom]:data-closed:slide-out-to-bottom-10 data-[side=left]:data-closed:slide-out-to-left-10 data-[side=right]:data-closed:slide-out-to-right-10 data-[side=top]:data-closed:slide-out-to-top-10',
			className
		)}
		{...restProps}
	>
		{@render children?.()}
		{#if showCloseButton}
			<ModalTooltip label="Close" class="absolute top-3 right-3">
				<SheetPrimitive.Close data-slot="sheet-close">
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
				</SheetPrimitive.Close>
			</ModalTooltip>
		{/if}
	</SheetPrimitive.Content>
</SheetPortal>
