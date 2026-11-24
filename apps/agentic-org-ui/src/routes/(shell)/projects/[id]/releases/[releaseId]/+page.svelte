<script lang="ts">
	import { page } from '$app/state';
	import { ReleasePage } from '$lib/modules/releases';

	const releaseId = $derived(page.params.releaseId ?? '');
</script>

{#key releaseId}
	<ReleasePage {releaseId} />
{/key}
