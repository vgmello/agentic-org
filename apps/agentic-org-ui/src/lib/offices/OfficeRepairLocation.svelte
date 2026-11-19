<script lang="ts">
	import type { OfficeInfo } from '$lib/proto';
	import { isHere } from './model';

	let { office }: { office: Pick<OfficeInfo, 'host' | 'endpoint'> } = $props();
</script>

{#if isHere(office)}
	This Mac{#if office.host !== 'This Mac'}{' (reported as '}<code class="font-mono break-all">{office.host}</code>){/if}
{:else}
	<code class="font-mono break-all">{office.host}</code>
{/if}
