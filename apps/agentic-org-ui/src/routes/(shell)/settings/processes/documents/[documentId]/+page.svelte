<script lang="ts">
	import { page } from '$app/state';
	import { editorMode } from '$lib/modules/processes';
	import DocumentEditor from '$lib/modules/processes/features/editor/DocumentEditor.svelte';

	let { data } = $props();
</script>

<!-- Another organization or document is another editor. The editor sets its own title. -->
{#key `${data.connection.organization.id}/${page.params.documentId}`}<DocumentEditor
		organizationId={data.connection.organization.id}
		mode={editorMode(page.url, page.params.documentId ?? '')}
	/>{/key}
