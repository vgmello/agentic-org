<!--
  @component
  WCAG 1.4.13 accessible tooltip (hoverable, persistent, Escape-dismissible) for icon-only controls
  inside modal Dialog/Sheet containers. Avoids Bits UI Tooltip.Content / PopperLayer, which mounts
  a FocusScope that pauses the enclosing modal focus trap.
-->
<script lang="ts">
	import { onDestroy, type Snippet } from 'svelte';
	import { cn } from '$lib/utils.js';

	let {
		label,
		side = 'bottom',
		class: className,
		children
	}: {
		label: string;
		side?: 'bottom' | 'left';
		class?: string;
		children: Snippet;
	} = $props();

	let open = $state(false);
	let hoverTimer: ReturnType<typeof setTimeout> | undefined;

	function clearTimer() {
		if (hoverTimer !== undefined) {
			clearTimeout(hoverTimer);
			hoverTimer = undefined;
		}
	}

	function scheduleOpen(event: PointerEvent) {
		if (event.pointerType === 'touch') return;
		clearTimer();
		hoverTimer = setTimeout(() => {
			hoverTimer = undefined;
			open = true;
		}, 150);
	}

	function close() {
		clearTimer();
		open = false;
	}

	function onFocusIn(event: FocusEvent) {
		const target = event.target;
		if (target instanceof HTMLElement && target.matches(':focus-visible')) {
			clearTimer();
			open = true;
		}
	}

	function onKeydownCapture(event: KeyboardEvent) {
		if (open && event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			close();
		}
	}

	function trackTooltip(node: HTMLElement) {
		node.addEventListener('pointerenter', scheduleOpen);
		node.addEventListener('pointerleave', close);
		node.addEventListener('focusin', onFocusIn);
		node.addEventListener('focusout', close);
		return {
			destroy() {
				clearTimer();
				node.removeEventListener('pointerenter', scheduleOpen);
				node.removeEventListener('pointerleave', close);
				node.removeEventListener('focusin', onFocusIn);
				node.removeEventListener('focusout', close);
			}
		};
	}

	onDestroy(clearTimer);
</script>

<svelte:window onkeydowncapture={onKeydownCapture} />

<span use:trackTooltip class={cn('relative inline-flex', className)}>
	{@render children()}
	{#if open}
		<span
			role="tooltip"
			data-tooltip-content=""
			class={cn(
				'absolute z-[60]',
				side === 'left' ? 'top-1/2 right-full -translate-y-1/2 pr-1.5' : 'top-full right-0 pt-1.5'
			)}
		>
			<span
				class="block whitespace-nowrap rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
				>{label}</span
			>
		</span>
	{/if}
</span>
