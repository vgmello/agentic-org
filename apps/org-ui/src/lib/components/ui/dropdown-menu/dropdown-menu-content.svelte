<script lang="ts">
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";
	import DropdownMenuPortal from "./dropdown-menu-portal.svelte";
	import { createMenuDispatch, type MenuValidityProps } from "./menu-dispatch.svelte";
	import { tick, type ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		sideOffset = 4,
		align = "start",
		preventScroll = false,
		isValidEvent,
		onInteractOutside,
		onCloseAutoFocus,
		portalProps,
		class: className,
		...restProps
	}: DropdownMenuPrimitive.ContentProps & MenuValidityProps & {
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof DropdownMenuPortal>>;
	} = $props();

	const dispatch = createMenuDispatch(() => ref);
	const validity: MenuValidityProps = { isValidEvent: (event, node) => isValidEvent?.(event, node) || dispatch.valid(event, node) };
	let pointerClose: { content: HTMLElement; target: HTMLElement | null; trigger: HTMLElement | null } | undefined;
	function interactOutside(event: PointerEvent) {
		pointerClose = undefined;
		onInteractOutside?.(event);
		if (event.defaultPrevented || !ref || dispatch.get(event)?.insideTree) return;
		const content = ref;
		const path = dispatch.get(event)?.path ?? event.composedPath();
		const target = path.find((node): node is HTMLElement => node instanceof HTMLElement
			&& node.matches('a[href],area[href],button,input:not([type="hidden"]),select,textarea,[tabindex],summary,[contenteditable=""],[contenteditable="true"],audio[controls],video[controls]')
			&& !node.matches(":disabled") && !node.closest("[inert]")) ?? null;
		pointerClose = { content, target, trigger: content.ownerDocument.querySelector<HTMLElement>(`[aria-controls~="${CSS.escape(content.id)}"]`) };
	}
	async function closeAutoFocus(event: Event) {
		const plan = pointerClose;
		pointerClose = undefined;
		onCloseAutoFocus?.(event);
		if (event.defaultPrevented || !plan) return;
		event.preventDefault();
		await tick();
		if (plan.content.dataset.state === "open") return;
		const active = plan.content.ownerDocument.activeElement;
		// Destination layers and explicit caller focus policies own their focus.
		if (active && active !== plan.content.ownerDocument.body && !plan.content.contains(active)) return;
		const target = plan.target ?? plan.trigger;
		target?.focus({ preventScroll: true });
	}
</script>

<DropdownMenuPortal {...portalProps}>
	<DropdownMenuPrimitive.Content
		bind:ref
		data-slot="dropdown-menu-content"
		{sideOffset}
		{align}
		{preventScroll}
		{...validity}
		onInteractOutside={interactOutside}
		onCloseAutoFocus={closeAutoFocus}
		class={cn(
			"data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 data-closed:zoom-out-95 data-open:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 ring-foreground/10 bg-popover text-popover-foreground min-w-32 rounded-md p-1 shadow-md ring-1 duration-100 z-50 max-h-(--bits-dropdown-menu-content-available-height) w-(--bits-dropdown-menu-anchor-width) origin-(--bits-dropdown-menu-content-transform-origin) overflow-x-hidden overflow-y-auto data-closed:overflow-hidden",
			className
		)}
		{...restProps}
	/>
</DropdownMenuPortal>
