<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import Ban from '@lucide/svelte/icons/ban';

	let {
		decisionId,
		actorName,
		busy,
		declineReason = $bindable(''),
		onConfirm,
		onCancel
	}: {
		decisionId: string;
		actorName: string;
		busy: boolean;
		declineReason: string;
		onConfirm: () => void;
		onCancel: () => void;
	} = $props();

	const focusDenyInput: Attachment<HTMLInputElement> = (el) => {
		el.focus();
	};

	function handleDenyKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			onConfirm();
		} else if (event.key === 'Escape') {
			event.preventDefault();
			onCancel();
		}
	}
</script>

<div class="deny-guard flex w-full flex-col gap-2 p-3" role="group" aria-label="Confirm deny">
	<div class="flex flex-wrap items-center justify-between gap-2.5">
		<span class="deny-guard-lbl inline-flex items-center gap-1.5 text-xs font-semibold">
			<Ban class="size-3.5 shrink-0" aria-hidden="true" />
			<span>Confirm deny?</span>
		</span>
		<div class="flex items-center gap-1.5">
			<button
				type="button"
				disabled={busy}
				onclick={onConfirm}
				class="deny-confirm inline-flex min-h-9 items-center rounded-md px-3 text-xs font-semibold"
			>
				Confirm
			</button>
			<button
				type="button"
				onclick={onCancel}
				class="skip-btn inline-flex min-h-9 items-center rounded-md px-2.5 text-xs font-semibold"
			>
				Keep
			</button>
		</div>
	</div>
	<label class="sr-only" for="deny-msg-{decisionId}">
		Optional reason for {actorName}
	</label>
	<input
		{@attach focusDenyInput}
		bind:value={declineReason}
		onkeydown={handleDenyKeydown}
		id="deny-msg-{decisionId}"
		type="text"
		autocomplete="off"
		placeholder="Optional reason for {actorName}… (Enter to confirm, Esc to cancel)"
		class="deny-input min-h-9 w-full rounded-md px-2.5 py-1.5 text-xs"
	/>
</div>

<style>
	.deny-guard {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-deny-guard-bg);
		box-shadow: inset 0 0 0 1px var(--exec-deny-guard-ring);
	}
	.deny-guard-lbl {
		color: var(--exec-deny-ink);
	}
	.deny-confirm {
		background: var(--exec-deny-confirm-bg);
		color: var(--exec-deny-confirm-ink);
	}
	.deny-confirm:hover {
		background: var(--exec-deny-confirm-hover);
	}
	.skip-btn {
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.skip-btn:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.deny-input {
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.deny-input::placeholder {
		color: var(--exec-muted);
		opacity: 1;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.skip-btn,
		.deny-confirm,
		.deny-input {
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.skip-btn {
			transition: none;
		}
	}
</style>
