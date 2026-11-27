<script lang="ts">
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import SendHorizontal from '@lucide/svelte/icons/send-horizontal';
	import { tick, untrack } from 'svelte';
	import {
		revokeStagedPreviews,
		stagedToAttachmentMeta,
		type StagedAttachment
	} from '$lib/components/attachments/model';
	import type { DraftText } from '../../drafts/store';
	import { Button } from '$lib/components/ui/button';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import {
		applyMention,
		cannotRunFor,
		cannotRunWords,
		mentionAt,
		mentionOptions,
		parseSlash,
		slashSuggestions
	} from '$lib/modules/messaging/model';
	import {
		CHAT_LIMITS,
		type AttachmentMeta,
		type Conversation
	} from '$lib/modules/messaging/types';
	import ComposerAttachments, { fileDropZone } from './ComposerAttachments.svelte';

	/**
	 * The message box (CHAT-14): `@` completes a name to the member's handle, `/` lists commands,
	 * Enter adds a line (⌘/Ctrl+Enter sends; "Enter sends" toggles), and Attach/paste/drop uploads files.
	 */
	let {
		placeholder,
		canWrite,
		note,
		cannotRun = [],
		conversation,
		ctx,
		resetKey,
		draft,
		alsoIn,
		onsend
	}: {
		placeholder: string;
		canWrite: boolean;
		note?: string;
		cannotRun?: readonly string[];
		conversation: Conversation;
		ctx: ChatContext;
		/** A draft is kept per key (a conversation or a thread). */
		resetKey: string;
		draft?: DraftText;
		/** A thread: the channel a reply can also be sent to. */
		alsoIn?: string;
		/** Resolves `false` when the message was not sent, so the text stays in the box. */
		onsend: (
			text: string,
			options: {
				alsoInChannel: boolean;
				attachments: string[];
				attachmentIds?: string[];
				attachmentMeta?: AttachmentMeta[];
				asMessage?: boolean;
			}
		) => Promise<boolean | void> | boolean | void;
	} = $props();

	const attachmentFocus =
		'focus-visible:outline-solid focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
	const drafts: Record<string, string> = {};
	const draftAttachments: Record<string, StagedAttachment[]> = {};
	let attachButton = $state<HTMLButtonElement>();
	let attachmentsBar = $state<ReturnType<typeof ComposerAttachments> | undefined>();
	let text = $state('');
	let caret = $state(0);
	let picked = $state(0);
	let dismissed = $state(false);
	let staged = $state<StagedAttachment[]>([]);
	let dragging = $state(false);
	let alsoInChannel = $state(false);
	let busy = $state(false);
	let box = $state<HTMLTextAreaElement | undefined>();
	let enterSends = $state(false);
	let lastKey = '';
	let lastDraft: DraftText | undefined;
	let error = $state<string | undefined>();
	let unknownCommand = $state(false);

	try {
		enterSends = globalThis.localStorage?.getItem('agentic-org.chat.enterSends') === '1';
	} catch {
		// no storage: the default
	}
	const setEnterSends = (on: boolean) => {
		enterSends = on;
		try {
			globalThis.localStorage?.setItem('agentic-org.chat.enterSends', on ? '1' : '0');
		} catch {
			// kept for this session
		}
	};

	// Conversation drafts live here. Thread drafts are supplied by the retained shell.
	$effect(() => {
		const key = resetKey;
		const nextDraft = draft;
		untrack(() => {
			if (lastKey && !lastDraft) {
				drafts[lastKey] = text;
				draftAttachments[lastKey] = staged;
			}
			lastKey = key;
			lastDraft = nextDraft;
			text = nextDraft ? nextDraft.read() : (drafts[key] ?? '');
			staged = nextDraft?.readAttachments
				? nextDraft.readAttachments()
				: (draftAttachments[key] ?? []);
			if (nextDraft && text) void restoreCaret(nextDraft, text);
			alsoInChannel = false;
			dismissed = false;
			dragging = false;
			error = undefined;
			unknownCommand = false;
		});
	});

	async function restoreCaret(restored: DraftText, value: string) {
		await tick();
		if (draft !== restored || text !== value || !box) return;
		caret = value.length;
		box.setSelectionRange(caret, caret);
	}

	const at = $derived(mentionAt(text, caret));
	const mentions = $derived(at && !dismissed ? mentionOptions(ctx, conversation, at.query) : []);
	const slashes = $derived(!dismissed && !at ? slashSuggestions(text) : []);
	const popup = $derived(mentions.length ? 'mention' : slashes.length ? 'slash' : undefined);
	const readyIds = $derived(
		staged.filter((item) => item.status === 'ready' && item.uploadId).map((item) => item.uploadId!)
	);
	const hasPendingUploads = $derived(staged.some((item) => item.status === 'uploading'));
	const hasFailedUploads = $derived(staged.some((item) => item.status === 'error'));
	const canSend = $derived(
		!busy && !hasPendingUploads && !hasFailedUploads && (text.trim() !== '' || readyIds.length > 0)
	);

	function syncStagedDraft(next: StagedAttachment[]) {
		if (draft?.writeAttachments) draft.writeAttachments(next);
		else draftAttachments[resetKey] = next;
	}

	function pickMention(handle: string) {
		if (!at) return;
		const next = applyMention(text, at, caret, handle);
		text = next.text;
		draft?.write(text);
		queueMicrotask(() => {
			box?.focus();
			box?.setSelectionRange(next.caret, next.caret);
			caret = next.caret;
		});
		picked = 0;
	}

	function pickSlash(name: string) {
		text = `/${name} `;
		draft?.write(text);
		queueMicrotask(() => {
			box?.focus();
			box?.setSelectionRange(text.length, text.length);
			caret = text.length;
		});
		picked = 0;
	}

	async function send(asMessage = false) {
		const body = text.trim();
		if (!canWrite || busy || hasPendingUploads) return;
		if (hasFailedUploads) {
			error = 'Retry or remove failed attachments before sending.';
			return;
		}
		if (body === '' && readyIds.length === 0) return;
		if (new TextEncoder().encode(body).byteLength > CHAT_LIMITS.bodyMax) {
			error = 'That message is too long. Shorten it or attach it as a file.';
			return;
		}
		const slash = asMessage || body === '' ? undefined : parseSlash(text);
		if (slash && !slash.ok && slash.unknown) {
			error = slash.message;
			unknownCommand = true;
			return;
		}
		error = undefined;
		unknownCommand = false;
		busy = true;
		const key = resetKey;
		const sendingDraft = draft;
		const clearDraft = sendingDraft?.clearOnSuccess();
		const submittedText = text;
		const submittedStaged = staged;
		const attachmentIds = readyIds;
		const attachmentNames = submittedStaged.map((item) => item.filename);
		const attachmentMeta = stagedToAttachmentMeta(
			submittedStaged,
			ctx.actor,
			new Date().toISOString()
		);
		try {
			if (
				(await onsend(submittedText, {
					alsoInChannel,
					attachments: attachmentNames,
					attachmentIds,
					attachmentMeta,
					asMessage
				})) === false
			) {
				return;
			}
			if (clearDraft) clearDraft();
			else {
				drafts[key] = '';
				draftAttachments[key] = [];
				revokeStagedPreviews(submittedStaged);
			}
			if (key === resetKey && sendingDraft === draft && text === submittedText) {
				text = '';
				attachmentsBar?.clear(false);
				staged = [];
				alsoInChannel = false;
			}
		} finally {
			busy = false;
			queueMicrotask(() => {
				if (key === resetKey && sendingDraft === draft) box?.focus();
			});
		}
	}

	function onkeydown(e: KeyboardEvent) {
		// ⌘ or Ctrl+Enter always sends, even with a list open.
		if (popup && !e.isComposing && !((e.metaKey || e.ctrlKey) && e.key === 'Enter')) {
			const n = popup === 'mention' ? mentions.length : slashes.length;
			if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
				e.preventDefault();
				picked = (picked + (e.key === 'ArrowDown' ? 1 : -1) + n) % n;
				return;
			}
			if (e.key === 'Enter' || e.key === 'Tab') {
				e.preventDefault();
				if (popup === 'mention') pickMention(mentions[picked % n].handle);
				else pickSlash(slashes[picked % n].name);
				return;
			}
			if (e.key === 'Escape') {
				e.preventDefault();
				dismissed = true;
				return;
			}
		}
		if (
			e.key === 'Enter' &&
			!e.isComposing &&
			(e.metaKey || e.ctrlKey || (enterSends && !e.shiftKey))
		) {
			e.preventDefault();
			void send();
		}
	}

	const bindDropZone = fileDropZone({
		canDrop: () => canWrite && !busy,
		onDragging: (active) => (dragging = active),
		onFiles: (files) => attachmentsBar?.addFiles(files)
	});

	const track = () => {
		caret = box?.selectionStart ?? text.length;
		dismissed = false;
		picked = 0;
	};
	const rows = $derived(Math.min(8, Math.max(2, text.split('\n').length)));
	const mentionBlocked = $derived(
		canWrite && cannotRun.length ? cannotRunFor(ctx, conversation, text, cannotRun) : []
	);
	const displayNote = $derived(
		note ?? (mentionBlocked.length ? cannotRunWords(ctx, mentionBlocked[0]) : undefined)
	);
</script>

<div
	{@attach bindDropZone}
	class="chat-compose border-t border-line bg-surface-sunken/30 px-4 py-3 transition-colors motion-reduce:transition-none {dragging
		? 'ring-2 ring-inset ring-focus bg-surface-raised'
		: ''}"
>
	{#if displayNote}<p
			class="mb-2 text-sm/6 {canWrite ? 'text-info-text-strong' : 'text-fg-secondary'}"
			data-testid="composer-note"
			aria-live="polite"
		>
			{displayNote}
		</p>{/if}
	{#if canWrite}
		<div class="relative">
			{#if popup}
				<ul
					id="composer-popup-{resetKey}"
					role="listbox"
					aria-label={popup === 'mention' ? 'Mention' : 'Commands'}
					class="absolute bottom-full z-20 mb-1 max-h-56 w-72 max-w-full overflow-y-auto rounded-md bg-surface-sunken py-1 shadow-lg ring-1 ring-line"
					data-testid="composer-popup"
				>
					{#if popup === 'mention'}
						{#each mentions as o, i (o.handle)}
							<li
								id="composer-opt-{resetKey}-{i}"
								role="option"
								aria-selected={i === picked % mentions.length}
							>
								<button
									type="button"
									class="flex w-full items-baseline gap-x-2 px-3 py-1.5 text-left text-sm/6 {i ===
									picked % mentions.length
										? 'bg-surface-overlay'
										: 'hover:bg-surface-raised'}"
									onmousedown={(e) => (e.preventDefault(), pickMention(o.handle))}
								>
									<span class="font-semibold text-fg">{o.name}</span><span class="text-fg-muted"
										>@{o.handle}</span
									>
								</button>
							</li>
						{/each}
					{:else}
						{#each slashes as c, i (c.name)}
							<li
								id="composer-opt-{resetKey}-{i}"
								role="option"
								aria-selected={i === picked % slashes.length}
							>
								<button
									type="button"
									class="flex w-full flex-col px-3 py-1.5 text-left text-sm/6 {i ===
									picked % slashes.length
										? 'bg-surface-overlay'
										: 'hover:bg-surface-raised'}"
									onmousedown={(e) => (e.preventDefault(), pickSlash(c.name))}
								>
									<span class="font-mono font-semibold text-fg">{c.usage}</span><span
										class="text-xs/5 text-fg-muted">{c.help}</span
									>
								</button>
							</li>
						{/each}
					{/if}
				</ul>
			{/if}
			<label class="sr-only" for="composer-{resetKey}">{placeholder}</label>
			<textarea
				id="composer-{resetKey}"
				bind:this={box}
				bind:value={text}
				aria-invalid={error ? true : undefined}
				aria-describedby={error ? `composer-error-${resetKey}` : undefined}
				aria-autocomplete="list"
				aria-controls={popup ? `composer-popup-${resetKey}` : undefined}
				aria-activedescendant={popup
					? `composer-opt-${resetKey}-${picked % (popup === 'mention' ? mentions.length : slashes.length)}`
					: undefined}
				{rows}
				{placeholder}
				maxlength={CHAT_LIMITS.bodyMax}
				disabled={busy}
				{onkeydown}
				onpaste={(event) => void attachmentsBar?.handlePaste(event)}
				oninput={(event) => {
					draft?.write(event.currentTarget.value);
					error = undefined;
					unknownCommand = false;
					track();
				}}
				onclick={track}
				onkeyup={(e) => (e.key.startsWith('Arrow') && popup ? undefined : track())}
				class="w-full resize-none rounded-md bg-surface px-3 py-2 text-sm/6 text-fg outline-1 -outline-offset-1 outline-field-border placeholder:text-fg-subtle focus-visible:outline-2 focus-visible:outline-focus"
				data-testid="composer"></textarea>
		</div>
		{#if error}<p id="composer-error-{resetKey}" class="mt-2 text-sm/6 text-danger" role="alert">
				<span>{error}</span>{#if unknownCommand}<button
						type="button"
						class="ml-1 font-semibold underline"
						disabled={busy}
						onclick={() => void send(true)}>Send as message</button
					>{/if}
			</p>{/if}
		<ComposerAttachments
			bind:this={attachmentsBar}
			bind:items={staged}
			disabled={busy}
			{resetKey}
			{attachButton}
			onItemsChange={syncStagedDraft}
		/>
		<div class="mt-2 flex flex-wrap items-center justify-between gap-2">
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs/5 text-fg-muted">
				<span><kbd class="font-mono">@</kbd> mention · <kbd class="font-mono">/</kbd> command</span>
				<button
					type="button"
					bind:this={attachButton}
					class="inline-flex items-center gap-x-1 hover:text-fg {attachmentFocus}"
					onclick={() => attachmentsBar?.openPicker()}
					><HeroIcon class="text-current" variant="mini" name="paper-clip" /> Attach</button
				>
				<label class="inline-flex items-center gap-x-1"
					><input
						type="checkbox"
						checked={enterSends}
						onchange={(e) => setEnterSends(e.currentTarget.checked)}
						class="accent-primary"
					/> Enter sends</label
				>
				{#if alsoIn}<label class="inline-flex items-center gap-x-1 text-fg-secondary"
						><input type="checkbox" bind:checked={alsoInChannel} class="accent-primary" /> Also send
						to {alsoIn}</label
					>{/if}
			</div>
			<Button
				type="button"
				size="sm"
				class="min-h-9 max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				disabled={!canSend}
				onclick={() => void send()}
				><SendHorizontal aria-hidden="true" /> Send<span class="sr-only"> (⌘ Enter)</span></Button
			>
		</div>
	{/if}
</div>

<style>
	@media (width < 640px), (height <= 32rem) {
		textarea {
			max-block-size: min(128px, calc(100dvh - var(--chat-header-height, 4rem)));
		}
	}
	@media (height <= 320px) {
		textarea {
			max-block-size: min(128px, calc(100dvh - 104px));
		}
	}
</style>
