<!--
  @component
  A replacement invitation, shown once (OFC-4): the same panel as Add office's, for New invitation on a row.
-->
<script lang="ts">
	import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import type { Issued } from '../../types';
	import InvitationPanel from './InvitationPanel.svelte';

	let { open = $bindable(false), issued, onCloseAutoFocus }: { open: boolean; issued: Issued | undefined; onCloseAutoFocus?: (event: Event) => void } = $props();
</script>

<Dialog bind:open>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-lg">
		<DialogHeader>
			<DialogTitle text={`New invitation for ${issued?.invitation.officeName ?? ''}`} />
			<DialogDescription>The previous invitation no longer works. Send this one to the machine that will run the office.</DialogDescription>
		</DialogHeader>
		{#if issued}<InvitationPanel {issued} ondone={() => (open = false)} />{/if}
	</DialogContent>
</Dialog>
