<script lang="ts">
	import { page } from '$app/state';
	import { ProjectFrame } from '$lib/modules/projects';

	let { data, children } = $props();
	const projectId = $derived(page.params.id ?? '');
</script>

<!-- Another project, or another organization, is another page: its reads start again. -->
{#key `${data.connection.organization.id}/${projectId}`}
	<ProjectFrame organizationId={data.connection.organization.id} {projectId} content={children} />
{/key}
