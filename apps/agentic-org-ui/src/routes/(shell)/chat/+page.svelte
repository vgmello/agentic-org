<script lang="ts">
	import ChatWorkspace from '$lib/modules/chat/features/workspace/ChatWorkspace.svelte';

	let { data } = $props();
</script>

<ChatWorkspace organizationId={data.connection.organization.id} snapshot={data.snapshot} />
