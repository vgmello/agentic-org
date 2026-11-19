<script lang="ts">
	import { page } from '$app/state';
	import { DocumentEditor, editorMode } from '$lib/modules/processes';

	let { data } = $props();
</script>

<!-- Another organization or starting point is another editor. The editor sets its own title. -->
{#key `${data.connection.organization.id}/${page.url.search}`}<DocumentEditor
		organizationId={data.connection.organization.id}
		mode={editorMode(page.url)}
	/>{/key}
