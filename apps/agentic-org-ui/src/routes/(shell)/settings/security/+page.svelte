<script lang="ts">
	import PageHeader from '$lib/components/PageHeader.svelte';
	import InstanceSecurityPanel from '$lib/modules/settings/features/security/InstanceSecurityPanel.svelte';
	import { pageTitle } from '$lib/nav';
</script>

<svelte:head><title>{pageTitle('Security & Access', 'Organization settings')}</title></svelte:head>

<PageHeader
	title="Security & Access"
	description="Enable password authentication and configure HTTPS for this HQ instance."
/>

<div class="px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
	<InstanceSecurityPanel />
</div>
