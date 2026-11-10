<script lang="ts">
	import Paperclip from '@lucide/svelte/icons/paperclip';
	import SendHorizontal from '@lucide/svelte/icons/send-horizontal';
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import { applyMention, mentionAt, mentionOptions, slashSuggestions } from '$lib/modules/messaging/model';
	import { CHAT_LIMITS, type Conversation } from '$lib/modules/messaging/types';

	/**
	 * The message box (CHAT-14): `@` completes a name to the member's handle, `/` lists the commands, Enter adds a line (⌘ or
	 * Ctrl+Enter sends; "Enter sends" in the corner turns that around), and 📎 attaches files by name in the sample.
	 */
	let {
		placeholder,
		canWrite,
		note,
		conversation,
		ctx,
		resetKey,
		alsoIn,
		onsend
	}: {
		placeholder: string;
		canWrite: boolean;
		note?: string;
		conversation: Conversation;
		ctx: ChatContext;
		/** A draft is kept per key (a conversation or a thread). */
		resetKey: string;
		/** A thread: the channel a reply can also be sent to. */
		alsoIn?: string;
		/** Resolves `false` when the message was not sent, so the text stays in the box. */
		onsend: (text: string, options: { alsoInChannel: boolean; attachments: string[] }) => Promise<boolean | void> | boolean | void;
	} = $props();

	const drafts: Record<string, string> = {};
	let text = $state('');
	let caret = $state(0);
	let picked = $state(0);
	let dismissed = $state(false);
	let attachments = $state<string[]>([]);
	let alsoInChannel = $state(false);
	let busy = $state(false);
	let box = $state<HTMLTextAreaElement | undefined>();
	let files = $state<HTMLInputElement | undefined>();
	let enterSends = $state(false);
	let lastKey = '';
	let error = $state<string | undefined>();

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

	// Keep a draft per conversation or thread.
	$effect(() => {
		const key = resetKey;
		untrack(() => {
			if (lastKey) drafts[lastKey] = text;
			lastKey = key;
			text = drafts[key] ?? '';
			attachments = [];
			alsoInChannel = false;
			dismissed = false;
			error = undefined;
		});
	});

	const at = $derived(mentionAt(text, caret));
	const mentions = $derived(at && !dismissed ? mentionOptions(ctx, conversation, at.query) : []);
	const slashes = $derived(!dismissed && !at ? slashSuggestions(text) : []);
	const popup = $derived(mentions.length ? 'mention' : slashes.length ? 'slash' : undefined);

	function pickMention(handle: string) {
		if (!at) return;
		const next = applyMention(text, at, caret, handle);
		text = next.text;
		queueMicrotask(() => {
			box?.focus();
			box?.setSelectionRange(next.caret, next.caret);
			caret = next.caret;
		});
		picked = 0;
	}
	function pickSlash(name: string) {
		text = `/${name} `;
		queueMicrotask(() => {
			box?.focus();
			box?.setSelectionRange(text.length, text.length);
			caret = text.length;
		});
		picked = 0;
	}

	async function send() {
		const body = text.trim();
		if (!canWrite || busy || body === '') return;
		if (new TextEncoder().encode(body).byteLength > CHAT_LIMITS.bodyMax) {
			error = 'That message is too long. Shorten it or attach it as a file.';
			return;
		}
		error = undefined;
		busy = true;
		const key = resetKey;
		try {
			if ((await onsend(body, { alsoInChannel, attachments })) === false) return;
			// The draft that was sent is gone, but only where it was written: the owner may have moved on while it went.
			drafts[key] = '';
			if (key === resetKey) {
				text = '';
				attachments = [];
				alsoInChannel = false;
			}
		} finally {
			busy = false;
			queueMicrotask(() => box?.focus());
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
		if (e.key === 'Enter' && !e.isComposing && (e.metaKey || e.ctrlKey || (enterSends && !e.shiftKey))) {
			e.preventDefault();
			void send();
		}
	}

	const track = () => {
		caret = box?.selectionStart ?? text.length;
		dismissed = false;
		picked = 0;
	};
	const rows = $derived(Math.min(8, Math.max(2, text.split('\n').length)));
</script>

<div class="border-t border-white/10 px-4 py-3">
	{#if note}<p class="mb-2 text-sm/6 {canWrite ? 'text-sky-200' : 'text-gray-300'}" data-testid="composer-note" aria-live="polite">{note}</p>{/if}
	{#if canWrite}
		<div class="relative">
			{#if popup}
				<ul id="composer-popup-{resetKey}" role="listbox" aria-label={popup === 'mention' ? 'Mention' : 'Commands'} class="absolute bottom-full z-20 mb-1 max-h-56 w-72 overflow-y-auto rounded-md bg-gray-900 py-1 shadow-lg ring-1 ring-white/10" data-testid="composer-popup">
					{#if popup === 'mention'}
						{#each mentions as o, i (o.handle)}
							<li id="composer-opt-{resetKey}-{i}" role="option" aria-selected={i === picked % mentions.length}>
								<button type="button" class="flex w-full items-baseline gap-x-2 px-3 py-1.5 text-left text-sm/6 {i === picked % mentions.length ? 'bg-white/10' : 'hover:bg-white/5'}" onmousedown={(e) => (e.preventDefault(), pickMention(o.handle))}>
									<span class="font-semibold text-white">{o.name}</span><span class="text-gray-400">@{o.handle}</span>
								</button>
							</li>
						{/each}
					{:else}
						{#each slashes as c, i (c.name)}
							<li id="composer-opt-{resetKey}-{i}" role="option" aria-selected={i === picked % slashes.length}>
								<button type="button" class="flex w-full flex-col px-3 py-1.5 text-left text-sm/6 {i === picked % slashes.length ? 'bg-white/10' : 'hover:bg-white/5'}" onmousedown={(e) => (e.preventDefault(), pickSlash(c.name))}>
									<span class="font-mono font-semibold text-white">{c.usage}</span><span class="text-xs/5 text-gray-400">{c.help}</span>
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
				aria-activedescendant={popup ? `composer-opt-${resetKey}-${picked % (popup === 'mention' ? mentions.length : slashes.length)}` : undefined}
				{rows}
				{placeholder}
				maxlength={CHAT_LIMITS.bodyMax}
				disabled={busy}
				{onkeydown}
				oninput={() => { error = undefined; track(); }}
				onclick={track}
				onkeyup={(e) => (e.key.startsWith('Arrow') && popup ? undefined : track())}
				class="w-full resize-none rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 placeholder:text-gray-500 focus-visible:outline-2 focus-visible:outline-indigo-400"
				data-testid="composer"
			></textarea>
		</div>
		{#if error}<p id="composer-error-{resetKey}" class="mt-2 text-sm/6 text-red-400" role="alert">{error}</p>{/if}
		{#if attachments.length}
			<ul class="mt-2 flex flex-wrap gap-1.5" aria-label="Attached">
				{#each attachments as a (a)}
					<li class="flex items-center gap-x-1 rounded bg-white/10 px-2 py-0.5 text-xs/5 text-gray-200">📎 {a} <button type="button" class="text-gray-400 hover:text-white" aria-label="Remove {a}" onclick={() => (attachments = attachments.filter((x) => x !== a))}>×</button></li>
				{/each}
			</ul>
		{/if}
		<div class="mt-2 flex flex-wrap items-center justify-between gap-2">
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs/5 text-gray-400">
				<span><kbd class="font-mono">@</kbd> mention · <kbd class="font-mono">/</kbd> command</span>
				<button type="button" class="inline-flex items-center gap-x-1 hover:text-white" onclick={() => files?.click()}><Paperclip class="size-3.5" aria-hidden="true" /> Attach</button>
				<input bind:this={files} type="file" multiple class="hidden" aria-label="Attach files" onchange={(e) => { attachments = [...new Set([...attachments, ...[...(e.currentTarget.files ?? [])].map((f) => f.name)])].slice(0, 10); e.currentTarget.value = ''; }} />
				<label class="inline-flex items-center gap-x-1"><input type="checkbox" checked={enterSends} onchange={(e) => setEnterSends(e.currentTarget.checked)} class="accent-indigo-500" /> Enter sends</label>
				{#if alsoIn}<label class="inline-flex items-center gap-x-1 text-gray-300"><input type="checkbox" bind:checked={alsoInChannel} class="accent-indigo-500" /> Also send to {alsoIn}</label>{/if}
			</div>
			<Button type="button" size="sm" disabled={busy || text.trim() === ''} onclick={send}><SendHorizontal aria-hidden="true" /> Send<span class="sr-only"> (⌘ Enter)</span></Button>
		</div>
	{/if}
</div>
