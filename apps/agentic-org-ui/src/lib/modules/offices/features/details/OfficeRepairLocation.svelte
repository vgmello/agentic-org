<script lang="ts">
	import type { OfficeInfo } from '$lib/proto';
	import { isHere } from '$lib/offices/model';

	let { office }: { office: Pick<OfficeInfo, 'host' | 'endpoint'> } = $props();
</script>

{#if isHere(office)}
	This Mac{#if office.host !== 'This Mac'}&#32;(reported as&#32;<code class="font-mono break-all"
			>{office.host}</code
		>){/if}
{:else}
	<code class="font-mono break-all">{office.host}</code>
{/if}
