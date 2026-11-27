<script lang="ts">
	import { page } from '$app/state';
	import { editorMode } from '$lib/modules/processes';
	import DocumentEditor from '$lib/modules/processes/features/editor/DocumentEditor.svelte';

	let { data } = $props();
</script>

<!-- Another organization or starting point is another editor. The editor sets its own title. -->
{#key `${data.connection.organization.id}/${page.url.search}`}<DocumentEditor
		organizationId={data.connection.organization.id}
		mode={editorMode(page.url)}
	/>{/key}
