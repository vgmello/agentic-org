<!--
  @component
  Footer bar with Create-more toggle, unknown-outcome recovery, Send to Tess, and primary submit for CaptureDialog.
-->
<script lang="ts">
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import { Button } from '$lib/components/ui/button';
	import { DialogFooter } from '$lib/components/ui/dialog';
	import { mobileButton } from '$lib/components/classes';
	import type { TaskType } from '../../types';

	let {
		type,
		busy,
		checking,
		unknownOpId,
		choicesUnavailable,
		bugSubmitDisabledReason,
		submitLabel,
		showSendToTess,
		createMore = $bindable(false),
		oncancel,
		ondiscard,
		oncheckstatus,
		onsendtotess
	}: {
		type: TaskType;
		busy: boolean;
		checking: boolean;
		unknownOpId: string | undefined;
		choicesUnavailable: boolean;
		bugSubmitDisabledReason: string | undefined;
		submitLabel: string;
		showSendToTess: boolean;
		createMore: boolean;
		oncancel: () => void;
		ondiscard: () => void;
		oncheckstatus: () => void;
		onsendtotess: () => void;
	} = $props();
</script>

<DialogFooter class="flex-wrap items-center justify-between gap-3 sm:justify-between">
	<label class="inline-flex cursor-pointer items-center gap-2 text-xs/5 text-fg-secondary">
		<button
			type="button"
			role="switch"
			aria-checked={createMore}
			aria-label="Create more"
			disabled={busy || checking || !!unknownOpId}
			onclick={() => (createMore = !createMore)}
			class="work-composer-switch"
			data-testid="composer-create-more"
		></button>
		<span>Create more</span>
	</label>
	<div class="flex flex-wrap items-center gap-2">
		<Button
			type="button"
			variant="outline"
			class={mobileButton}
			disabled={busy || checking}
			onclick={oncancel}>Cancel</Button
		>
		{#if unknownOpId}
			<Button
				type="button"
				variant="outline"
				class={mobileButton}
				disabled={checking}
				onclick={ondiscard}>Discard attempt</Button
			>
			<Button type="button" class={mobileButton} disabled={checking} onclick={oncheckstatus}>
				{checking ? 'Checking…' : 'Check status'}
			</Button>
		{:else}
			{#if type !== 'bug' && showSendToTess}
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy || choicesUnavailable}
					onclick={onsendtotess}
					data-testid="composer-send-to-tess"
				>
					<Sparkles class="size-3.5" aria-hidden="true" />
					<span>Send to Tess</span>
				</Button>
			{/if}
			<Button
				type="submit"
				class={mobileButton}
				disabled={busy || choicesUnavailable || !!bugSubmitDisabledReason}
				aria-busy={busy}
			>
				{submitLabel}
			</Button>
		{/if}
	</div>
</DialogFooter>

<style>
	.work-composer-switch {
		position: relative;
		width: 2rem;
		height: 1.125rem;
		border-radius: 9999px;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		background: var(--exec-well, var(--surface-raised));
		cursor: pointer;
		transition: background-color 140ms ease-out;
	}
	.work-composer-switch::after {
		content: '';
		position: absolute;
		top: 2px;
		left: 2px;
		width: 0.75rem;
		height: 0.75rem;
		border-radius: 9999px;
		background: var(--exec-muted, var(--fg-muted));
		transition: transform 140ms ease-out;
	}
	.work-composer-switch[aria-checked='true'] {
		background: var(--tone-accent, var(--primary));
		border-color: var(--tone-accent, var(--primary));
	}
	.work-composer-switch[aria-checked='true']::after {
		transform: translateX(0.875rem);
		background: var(--on-emphasis);
	}
	.work-composer-switch:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (prefers-reduced-motion: reduce) {
		.work-composer-switch,
		.work-composer-switch::after {
			transition: none;
		}
	}
</style>
