<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/office/api';
	import { owner } from '$lib/org/model';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import InboxTriageWorkspace from '$lib/modules/inbox/features/triage/InboxTriageWorkspace.svelte';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const ownerId = $derived(owner(snapshot)?.id ?? 'owner');
	const shellData = useShellData();

	const ready = $derived(shellData.loaded && shellData.loadedOrganizationId === organizationId);
	const workItems = $derived(ready ? shellData.workItems : []);
	const inboxItems = $derived(ready ? shellData.inboxItems : []);

	let lastReportedErrorSeq = untrack(() => (shellData.loaded ? shellData.workErrorSeq : 0));

	$effect(() => {
		const id = organizationId;
		if (untrack(() => shellData.loaded && shellData.loadedOrganizationId === id)) {
			void shellData.refresh();
		}
	});

	$effect(() => {
		const seq = shellData.workErrorSeq;
		const rawError = shellData.workError;
		if (!rawError || seq === lastReportedErrorSeq) return;
		lastReportedErrorSeq = seq;
		const err = toAppError(rawError);
		if (err.source === 'api' && (err.code === 'unsupported' || err.code === 'unavailable')) return;
		toast.error(err.message);
	});
</script>

<svelte:head><title>{pageTitle('Inbox')}</title></svelte:head>

<InboxTriageWorkspace
	{organizationId}
	{snapshot}
	{ownerId}
	{workItems}
	{inboxItems}
	loading={!ready}
	onRefresh={() => shellData.refresh()}
/>
