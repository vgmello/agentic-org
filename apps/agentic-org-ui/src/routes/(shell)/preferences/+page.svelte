<script lang="ts">
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { KeyboardSection, TimeZoneSection } from '$lib/modules/preferences';
	import { pageTitle } from '$lib/nav';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Your preferences')}</title></svelte:head>

<PageHeader title="Your preferences" description="Settings for you, not for the organization." />
<div class="divide-y divide-line-subtle">
	<KeyboardSection />
	<TimeZoneSection organizationId={data.connection.organization.id} />
</div>
