<script lang="ts">
	import ConfirmDialog from '$lib/modules/settings/components/ConfirmDialog.svelte';
	import { officesApi } from '../../api';
	import { removeOfficeText } from '../../copy';

	let {
		open = $bindable(false),
		office,
		organizationId,
		organizationName,
		agents,
		tasks,
		offline,
		onchanged,
		onCloseAutoFocus
	}: {
		open: boolean;
		office: { id: string; name: string } | undefined;
		organizationId: string;
		organizationName: string;
		agents: number;
		tasks: number;
		offline: boolean;
		onchanged: () => void | Promise<void>;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let shown = $state<{ id: string; name: string } | undefined>();
	$effect(() => {
		if (open && office) shown = office;
	});
	const text = $derived(removeOfficeText({ office: shown?.name ?? '', organization: organizationName, agents, tasks, offline }));
</script>

<ConfirmDialog
	bind:open
	title={`Remove ${shown?.name ?? ''} from ${organizationName}?`}
	description={text.replace(/^[^?]*\? /, '')}
	confirmLabel={`Remove ${shown?.name ?? ''}`}
	busyLabel="Removing…"
	destructive
	{onCloseAutoFocus}
	onconfirm={async () => {
		if (!shown) return;
		await officesApi.removeOffice(organizationId, shown.id);
		await onchanged();
	}}
/>
