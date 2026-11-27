<script lang="ts">
	import Ban from '@lucide/svelte/icons/ban';

	let {
		itemId,
		actorName,
		declineReason,
		isBusy,
		controlsDisabled,
		denyInputEl = $bindable(null),
		onDeclineReasonChange,
		onCancel,
		onConfirm
	}: {
		itemId: string;
		actorName: string;
		declineReason: string;
		isBusy: boolean;
		controlsDisabled: boolean;
		denyInputEl?: HTMLInputElement | null;
		onDeclineReasonChange: (value: string) => void;
		onCancel: () => void;
		onConfirm: () => void;
	} = $props();

	const btnTouch =
		'[@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11';

	function denyGuardEscape(node: HTMLElement) {
		const listener = (event: KeyboardEvent) => {
			if (event.isComposing || event.key !== 'Escape') return;
			event.preventDefault();
			event.stopPropagation();
			onCancel();
		};
		node.addEventListener('keydown', listener);
		return () => node.removeEventListener('keydown', listener);
	}

	function handleKeydown(event: KeyboardEvent) {
		if (!event.isComposing && event.key === 'Enter') {
			event.preventDefault();
			onConfirm();
		}
	}
</script>

<div
	{@attach denyGuardEscape}
	class="triage-deny-guard flex w-full flex-col gap-2.5 p-3.5"
	role="group"
	aria-label="Confirm deny"
>
	<div class="flex flex-wrap items-center justify-between gap-2">
		<span class="triage-deny-label inline-flex items-center gap-1.5 text-xs font-semibold">
			<Ban class="size-3.5 shrink-0" aria-hidden="true" />
			<span>Confirm deny?</span>
		</span>
		<div class="flex items-center gap-2">
			<button
				type="button"
				disabled={controlsDisabled}
				onclick={onCancel}
				class="triage-keep-btn inline-flex min-h-9 min-w-9 items-center justify-center px-3 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
			>
				Keep
			</button>
			<button
				type="button"
				disabled={controlsDisabled}
				onclick={onConfirm}
				class="triage-deny-confirm inline-flex min-h-9 min-w-9 items-center justify-center px-3 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
			>
				{isBusy ? 'Denying…' : 'Confirm deny'}
			</button>
		</div>
	</div>
	<label class="sr-only" for="triage-deny-reason-{itemId}">
		Optional reason for {actorName}
	</label>
	<input
		bind:this={denyInputEl}
		value={declineReason}
		oninput={(e) => onDeclineReasonChange(e.currentTarget.value)}
		onkeydown={handleKeydown}
		disabled={controlsDisabled}
		id="triage-deny-reason-{itemId}"
		type="text"
		autocomplete="off"
		placeholder="Optional reason for {actorName}… (Enter to confirm, Esc to cancel)"
		class="triage-input min-h-9 w-full px-3 py-1.5 text-xs disabled:cursor-not-allowed disabled:opacity-60 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
	/>
</div>

<style>
	.triage-deny-guard {
		border-radius: var(--exec-inset-radius);
		background: var(--exec-deny-guard-bg);
		box-shadow: inset 0 0 0 1px var(--exec-deny-guard-ring);
	}
	.triage-deny-label {
		color: var(--exec-deny-ink);
	}
	.triage-keep-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		cursor: pointer;
	}
	.triage-keep-btn:hover:not(:disabled) {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	.triage-deny-confirm {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-deny-confirm-bg);
		color: var(--exec-deny-confirm-ink);
		cursor: pointer;
	}
	.triage-deny-confirm:hover:not(:disabled) {
		background: var(--exec-deny-confirm-hover);
	}
	.triage-input {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.triage-input::placeholder {
		color: var(--exec-muted);
		opacity: 1;
	}
	.triage-input:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	.triage-keep-btn:focus-visible,
	.triage-deny-confirm:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (forced-colors: active) {
		.triage-deny-guard,
		.triage-keep-btn,
		.triage-deny-confirm {
			border: 1px solid ButtonText;
		}
		.triage-input {
			border: 1px solid CanvasText;
		}
	}
</style>
