<!--
  @component
  A confirmation that names the object and the effect (app-shell-ux §6a). The confirm button reads its -ing word while it works and
  stays the same width; a refusal shows inline and keeps the dialog open. Cancel and Escape do nothing while it works.
  Usage: <ConfirmDialog bind:open title="Decline Studio Mac?" description="Its invitation is cancelled." confirmLabel="Decline" busyLabel="Declining…" onconfirm={decline} />
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { toAppError } from '$lib/office/api';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { dangerButton, mobileButton } from './classes';

	let {
		open = $bindable(false),
		title,
		description,
		body,
		confirmLabel,
		busyLabel,
		cancelLabel = 'Cancel',
		destructive = false,
		onconfirm,
		onCloseAutoFocus
	}: {
		open: boolean;
		title: string;
		description?: string;
		/** More than a sentence: the effects, one per line. */
		body?: Snippet;
		confirmLabel: string;
		busyLabel: string;
		cancelLabel?: string;
		destructive?: boolean;
		/** Resolve to close; reject to show the reason and stay. */
		onconfirm: () => Promise<void>;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let busy = $state(false);
	let error = $state<string | undefined>();
	$effect(() => {
		if (open) error = undefined;
	});

	async function confirm() {
		if (busy) return;
		busy = true;
		error = undefined;
		try {
			await onconfirm();
			// Stays busy until the close has finished, so the button never flips back while it fades out.
			open = false;
		} catch (e) {
			error = toAppError(e).message;
			busy = false;
		}
	}
</script>

<Dialog
	bind:open
	onOpenChange={(next) => !next && busy && (open = true)}
	onOpenChangeComplete={(next) => !next && (busy = false)}
>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<DialogHeader>
			<DialogTitle text={title} />
			{#if description}<DialogDescription class="[overflow-wrap:anywhere]"
					>{description}</DialogDescription
				>{/if}
		</DialogHeader>
		{#if body}<div class="space-y-2 text-sm/6 text-gray-300 [overflow-wrap:anywhere]">
				{@render body()}
			</div>{/if}
		{#if error}<p class="text-sm/6 text-red-300 [overflow-wrap:anywhere]" role="alert">
				{error}
			</p>{/if}
		<DialogFooter>
			<Button
				type="button"
				variant="outline"
				class={mobileButton}
				disabled={busy}
				onclick={() => (open = false)}>{cancelLabel}</Button
			>
			<Button
				type="button"
				class="{mobileButton} {destructive ? dangerButton : ''}"
				aria-busy={busy}
				aria-disabled={busy}
				onclick={confirm}>{busy ? busyLabel : confirmLabel}</Button
			>
		</DialogFooter>
	</DialogContent>
</Dialog>
