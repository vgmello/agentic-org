<script lang="ts">
	import { page } from '$app/state';
	import { FeaturePage } from '$lib/modules/features';

	const featureId = $derived(page.params.featureId ?? '');
</script>

{#key featureId}
	<FeaturePage {featureId} />
{/key}
