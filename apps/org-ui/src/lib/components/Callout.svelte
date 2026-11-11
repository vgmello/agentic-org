<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';
	import * as Alert from './ui/alert';
	import { cn } from '$lib/utils';

	let { role, children, icon, class: className, ...attributes }: HTMLAttributes<HTMLDivElement> & { children: Snippet; icon?: Snippet } = $props();
</script>

<!-- Static context is quiet; the caller explicitly opts into alert/status announcements. -->
<Alert.Root {role} class={cn('flex min-w-0 flex-wrap items-start gap-x-2.5 gap-y-2 border-white/10 bg-white/[0.03] text-gray-300 [overflow-wrap:anywhere]', className)} {...attributes}>
	{#if icon}<span class="shrink-0" aria-hidden="true">{@render icon()}</span>{/if}
	<div class="min-w-0 max-w-full flex-1 basis-[12ch]">{@render children()}</div>
</Alert.Root>
