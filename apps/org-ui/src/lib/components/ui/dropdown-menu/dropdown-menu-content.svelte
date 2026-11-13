<script lang="ts">
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";
	import { returnFocus } from "$lib/a11y/focus-return.js";
	import DropdownMenuPortal from "./dropdown-menu-portal.svelte";
	import { getMenuReturn } from "./dropdown-menu-return.svelte.js";
	import type { ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		sideOffset = 4,
		align = "start",
		portalProps,
		class: className,
		onOpenAutoFocus,
		onInteractOutside,
		onCloseAutoFocus,
		...restProps
	}: DropdownMenuPrimitive.ContentProps & {
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof DropdownMenuPortal>>;
	} = $props();

	// A click outside the menu closes it without Bits giving focus back (in every engine, and WebKit never focuses
	// a clicked button). Escape, choosing an item and Tab keep Bits' own behaviour; only the outside click is handled here.
	const menu = getMenuReturn();
	const openFocus = (e: Event) => {
		if (menu) menu.outsideClose = false;
		onOpenAutoFocus?.(e);
	};
	const outside = (e: PointerEvent) => {
		onInteractOutside?.(e);
		if (menu && !e.defaultPrevented) menu.outsideClose = true;
	};
	const closeFocus = (e: Event) => {
		onCloseAutoFocus?.(e);
		if (!menu || e.defaultPrevented || !menu.outsideClose) return;
		menu.outsideClose = false;
		e.preventDefault();
		void returnFocus(menu.trigger, { within: ref });
	};
</script>

<DropdownMenuPortal {...portalProps}>
	<DropdownMenuPrimitive.Content
		bind:ref
		data-slot="dropdown-menu-content"
		{sideOffset}
		{align}
		onOpenAutoFocus={openFocus}
		onInteractOutside={outside}
		onCloseAutoFocus={closeFocus}
		class={cn(
			"data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 data-closed:zoom-out-95 data-open:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 ring-foreground/10 bg-popover text-popover-foreground min-w-32 rounded-md p-1 shadow-md ring-1 duration-100 z-50 max-h-(--bits-dropdown-menu-content-available-height) w-(--bits-dropdown-menu-anchor-width) origin-(--bits-dropdown-menu-content-transform-origin) overflow-x-hidden overflow-y-auto data-closed:overflow-hidden",
			className
		)}
		{...restProps}
	/>
</DropdownMenuPortal>
