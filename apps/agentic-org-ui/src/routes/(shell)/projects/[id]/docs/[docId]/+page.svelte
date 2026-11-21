<script lang="ts">
	import { page } from '$app/state';
	import { DocumentPage } from '$lib/modules/docs';

	const docId = $derived(page.params.docId ?? '');
</script>

{#key docId}
	<DocumentPage {docId} />
{/key}
