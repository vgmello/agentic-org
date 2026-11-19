<script lang="ts">
	import { page } from '$app/state';
	import { DocumentEditor, editorMode } from '$lib/modules/processes';

	let { data } = $props();
</script>

<!-- Another organization or document is another editor. The editor sets its own title. -->
{#key `${data.connection.organization.id}/${page.params.documentId}`}<DocumentEditor
		organizationId={data.connection.organization.id}
		mode={editorMode(page.url, page.params.documentId ?? '')}
	/>{/key}
