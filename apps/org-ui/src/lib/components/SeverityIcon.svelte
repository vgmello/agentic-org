<script lang="ts">
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import Info from '@lucide/svelte/icons/info';
	import Minus from '@lucide/svelte/icons/minus';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import type { Severity } from '$lib/proto';

	let { severity }: { severity: Severity | undefined } = $props();
</script>

<!-- The icon beside a severity word (the word is always there too). -->
{#if severity === 'critical'}<TriangleAlert class="inline size-3.5 text-amber-300" aria-hidden="true" />
{:else if severity === 'major'}<CircleAlert class="inline size-3.5 text-gray-200" aria-hidden="true" />
{:else if severity === 'minor'}<Info class="inline size-3.5 text-gray-400" aria-hidden="true" />
{:else}<Minus class="inline size-3.5 text-gray-500" aria-hidden="true" />{/if}
