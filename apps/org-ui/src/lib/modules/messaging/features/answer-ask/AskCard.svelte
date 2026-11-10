<script lang="ts">
	import { tick } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import { acceptedWords, answerWords, appOnly, deadlineWords, nameOf, waitingFor } from '$lib/modules/messaging/model';
	import type { ChatMessage } from '$lib/modules/messaging/types';
	import { collapse } from '$lib/modules/messaging/model';
	import Rich from '../../components/Rich.svelte';

	/**
	 * An ask, an approval or a hand-off as a card (CHAT-5, CHAT-6, CHAT-17). Answering here answers the ask. Where the card is
	 * `readOnly` (a bridge) an ask to the owner and every approval show **Open in Agentic Org** instead of buttons.
	 */
	let {
		message,
		ctx,
		now,
		readOnly = false,
		officeRunning = true,
		where,
		onanswer,
		onaccept,
		onreplyinthread
	}: {
		message: ChatMessage;
		ctx: ChatContext;
		now: string;
		readOnly?: boolean;
		officeRunning?: boolean;
		/** Shown on cards outside their conversation (Needs you, Mentions): "in #atlas". */
		where?: string;
		onanswer: (answer: { option_id?: string; text?: string }) => Promise<void> | void;
		onaccept: () => Promise<void> | void;
		onreplyinthread?: () => void;
	} = $props();

	let expanded = $state(false);
	let replying = $state(false);
	let rejecting = $state(false);
	let text = $state('');
	let busy = $state(false);
	let error = $state<string | undefined>();

	const long = $derived(collapse(message.body));
	const isAsk = $derived(message.kind === 'request');
	const to = $derived(message.addressed_to === ctx.owner ? 'you' : nameOf(ctx, message.addressed_to ?? ''));
	const deadline = $derived(deadlineWords(message, now));
	const blockedHere = $derived(readOnly && appOnly(message, ctx));
	const done = $derived(isAsk ? message.answer : message.accepted_by);
	// Only the hand-off's recipient accepts (the owner when it is handed to the owner); everyone else, the owner reading as the
	// owner included, can only reply (CHAT-18).
	const canAccept = $derived(message.addressed_to === ctx.actor);

	async function go(run: () => Promise<void> | void) {
		if (!officeRunning || busy) return;
		const origin = document.activeElement;
		busy = true;
		error = undefined;
		try {
			await run();
			replying = rejecting = false;
			text = '';
		} catch (e) {
			error = (e as { message?: string }).message ?? 'That did not go through.';
		} finally {
			busy = false;
			await tick();
			// A delayed refusal can blur a disabled control. Restore it only if focus was lost here.
			if (error && origin instanceof HTMLElement && origin.isConnected && document.activeElement === document.body) origin.focus({ preventScroll: true });
		}
	}
</script>

<div tabindex="-1" data-ask-id={message.id} class="rounded-lg border {done ? 'border-white/10 bg-white/[0.03]' : isAsk ? 'border-amber-400/40 bg-amber-400/5' : 'border-indigo-400/30 bg-indigo-400/5'} p-4" data-testid={isAsk ? 'ask-card' : 'handoff-card'} data-answered={done ? 'true' : 'false'}>
	<p class="text-xs/5 font-semibold {isAsk ? 'text-amber-200' : 'text-indigo-200'}">
		{#if isAsk}{message.approval ? '✅ Approval' : '❓ Ask'}: {nameOf(ctx, message.from)} asks {to}{:else}🔁 Hand-off: {nameOf(ctx, message.from)} → {to}{/if}
		{#if where}<span class="font-normal text-gray-400">{' · in ' + where}</span>{/if}
	</p>
	<div class="mt-2">
		<Rich body={expanded ? message.body : long.shown} />
		{#if long.hidden > 0}<button type="button" class="mt-1 text-sm/6 font-semibold text-indigo-300 hover:text-indigo-200" aria-expanded={expanded} onclick={() => (expanded = !expanded)}>{expanded ? 'Show less' : `Show more (${long.hidden} more lines)`}</button>{/if}
	</div>

	{#if isAsk && !message.answer}
		<p class="mt-2 text-xs/5 text-gray-400">
			<span data-testid="waiting">{waitingFor(message, now)}</span>{#if message.blocks}{' · blocks ' + message.blocks}{/if}
		</p>
		{#if deadline}<p class="mt-1 text-xs/5 text-amber-100" data-testid="deadline">{deadline.text}</p>{/if}
	{/if}

	{#if done}
		<p class="mt-3 text-sm/6 text-emerald-300" data-testid="answered">{isAsk ? answerWords(message, ctx) : acceptedWords(message, ctx)}</p>
		{#if isAsk && message.answer?.by === 'default' && deadline}<p class="text-xs/5 text-gray-400" data-testid="deadline">{deadline.text}</p>{/if}
	{:else if !officeRunning}
		<p class="mt-3 rounded-md bg-white/5 px-3 py-2 text-sm/6 text-gray-300">The office isn't running. Restart it from Offices.</p>
	{:else if blockedHere}
		<p class="mt-3 rounded-md bg-white/5 px-3 py-2 text-sm/6 text-gray-300" data-testid="app-only">This is answered only in Agentic Org, never from a bridge. <a href="/chat" class="font-semibold text-indigo-300 underline">Open in Agentic Org</a></p>
	{:else if isAsk}
		<div class="mt-3 flex flex-wrap items-center gap-2">
			{#each message.options ?? [] as o (o.id)}
				{#if message.approval && o.id === 'reject'}
					<Button type="button" size="sm" variant="outline" disabled={busy} onclick={() => ((rejecting = !rejecting), (replying = false))}>{o.label}</Button>
				{:else}
					<Button type="button" size="sm" variant={message.approval ? 'default' : 'outline'} disabled={busy} onclick={() => go(() => onanswer({ option_id: o.id }))}>{o.label}</Button>
				{/if}
			{/each}
			{#if !message.approval}
				<Button type="button" size="sm" variant="ghost" disabled={busy} onclick={() => ((replying = !replying), (rejecting = false))}>Reply…</Button>
			{/if}
		</div>
		{#if replying || rejecting}
			<form class="mt-2 flex flex-wrap items-start gap-2" onsubmit={(e) => (e.preventDefault(), go(() => onanswer(rejecting ? { option_id: 'reject', text } : { text })))}>
				<label class="sr-only" for="ask-text-{message.id}">{rejecting ? 'Why do you reject it?' : 'Your reply'}</label>
				<textarea id="ask-text-{message.id}" bind:value={text} rows="2" maxlength={20000} disabled={busy} placeholder={rejecting ? 'Why do you reject it?' : 'Your reply'} class="min-w-0 flex-1 rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400"></textarea>
				<Button type="submit" size="sm" disabled={busy || text.trim() === ''}>{rejecting ? 'Reject' : 'Send reply'}</Button>
			</form>
		{/if}
	{:else}
		<div class="mt-3 flex flex-wrap items-center gap-2">
			{#if canAccept}<Button type="button" size="sm" variant="outline" disabled={busy} onclick={() => go(onaccept)}>Accept</Button>{/if}
			{#if onreplyinthread}<Button type="button" size="sm" variant="ghost" onclick={onreplyinthread}>Reply…</Button>{/if}
			{#if message.linked_ref}<div><Rich body={`Linked to ${message.linked_ref}`} /></div>{/if}
		</div>
	{/if}
	{#if error}<p class="mt-2 text-sm/6 text-red-400" role="alert">{error}</p>{/if}
</div>
