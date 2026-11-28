<script lang="ts">
	import { provideAttachmentOrgId } from '$lib/components/attachments/context';
	import ChatWorkspace from '$lib/modules/chat/features/workspace/ChatWorkspace.svelte';

	let { data } = $props();

	provideAttachmentOrgId(() => data.connection.organization.id);
</script>

<ChatWorkspace organizationId={data.connection.organization.id} snapshot={data.snapshot} />
