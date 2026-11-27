<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import MembersDirectory from '$lib/modules/members/features/directory/MembersDirectory.svelte';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Members', 'Teams')}</title></svelte:head>

<MembersDirectory
	organizationId={data.connection.organization.id}
	organizationName={data.connection.organization.name}
	snapshot={data.snapshot}
	loads={data.loads}
/>
