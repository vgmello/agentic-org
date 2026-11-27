<script lang="ts">
	import { page } from '$app/state';
	import FeaturePage from '$lib/modules/features/features/detail/FeaturePage.svelte';

	const featureId = $derived(page.params.featureId ?? '');
</script>

{#key featureId}
	<FeaturePage {featureId} />
{/key}
