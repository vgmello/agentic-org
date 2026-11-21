<script lang="ts">
	import { ChatWorkspace } from '$lib/modules/chat';

	let { data } = $props();
</script>

<ChatWorkspace organizationId={data.connection.organization.id} snapshot={data.snapshot} />
