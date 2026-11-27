<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import Send from '@lucide/svelte/icons/send';

	interface Props {
		decisionId: string;
		actorName: string;
		busy: boolean;
		replyText: string;
		onSubmit: () => void;
		onCancel: () => void;
	}

	let {
		decisionId,
		actorName,
		busy,
		replyText = $bindable(''),
		onSubmit,
		onCancel
	}: Props = $props();

	const focusReplyInput: Attachment<HTMLTextAreaElement> = (el) => {
		el.focus();
	};

	const canSubmit = $derived(!busy && replyText.trim().length > 0);

	function handleKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			event.preventDefault();
			onCancel();
			return;
		}
		const hasModifier = event.metaKey || event.ctrlKey;
		if (hasModifier && event.key === 'Enter' && canSubmit) {
			event.preventDefault();
			onSubmit();
		}
	}
</script>

<div class="reply-box flex w-full flex-col gap-2.5" role="group" aria-label="Answer {actorName}">
	<label class="sr-only" for="spot-reply-{decisionId}">Answer {actorName}</label>
	<textarea
		{@attach focusReplyInput}
		id="spot-reply-{decisionId}"
		bind:value={replyText}
		onkeydown={handleKeydown}
		rows="2"
		placeholder="Answer {actorName}… (⌘/Ctrl+Enter to send, Esc to cancel)"
		class="reply-input w-full rounded-lg px-3 py-2 text-sm"></textarea>
	<div class="flex flex-wrap items-center gap-2">
		<button type="button" disabled={!canSubmit} onclick={onSubmit} class="exec-cta min-h-9">
			<span class="exec-cta-fill" aria-hidden="true"></span>
			<span class="exec-cta-sheen" aria-hidden="true"></span>
			<span class="exec-cta-inner">
				<Send class="size-3.5" aria-hidden="true" />
				<span>Send answer</span>
			</span>
		</button>
		<button
			type="button"
			disabled={busy}
			onclick={onCancel}
			class="cancel-btn inline-flex min-h-9 items-center rounded-md px-3 text-xs font-semibold"
		>
			Cancel
		</button>
	</div>
</div>

<style>
	.reply-input {
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		resize: vertical;
		min-height: 68px;
	}
	.reply-input::placeholder {
		color: var(--exec-muted);
		opacity: 1;
	}
	.reply-input:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -1px;
	}
	.cancel-btn {
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.cancel-btn:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.cancel-btn {
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.cancel-btn {
			transition: none;
		}
	}
</style>
