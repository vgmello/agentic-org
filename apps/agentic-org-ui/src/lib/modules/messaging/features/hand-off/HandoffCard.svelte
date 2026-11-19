<script lang="ts">
	import { tick } from 'svelte';
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import { Button } from '$lib/components/ui/button';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import { appOnly, collapse, handoffOutcome, nameOf, whoOf } from '$lib/modules/messaging/model';
	import type { ChatMessage } from '$lib/modules/messaging/types';
	import Rich from '../../components/Rich.svelte';

	/**
	 * A hand-off (chat-ux §2, §2.2; CHAT-18, CHAT-39): "Noor Haddad → you". Only its recipient (the owner when it is handed to
	 * the owner) sees **Accept** and **Hand back…** (a reason is required); everyone sees **Reply…**. Once accepted it reads
	 * "Accepted by <name> · <time>" with no buttons. Accepting records the acceptance only: it never closes the linked task.
	 */
	let {
		message,
		ctx,
		readOnly = false,
		officeRunning = true,
		where,
		onaccept,
		onhandback,
		onreplyinthread
	}: {
		message: ChatMessage;
		ctx: ChatContext;
		readOnly?: boolean;
		officeRunning?: boolean;
		where?: string;
		onaccept: () => Promise<void> | void;
		onhandback?: (reason: string) => Promise<void> | void;
		onreplyinthread?: (opener: HTMLElement) => void;
	} = $props();

	let expanded = $state(false);
	let handingBack = $state(false);
	let reason = $state('');
	let busy = $state(false);
	let error = $state<string | undefined>();
	let reasonBox = $state<HTMLTextAreaElement>();

	const long = $derived(collapse(message.body));
	const outcome = $derived(handoffOutcome(message, ctx));
	const closed = $derived(!!outcome);
	const canAccept = $derived(
		message.addressed_to === ctx.actor && !(readOnly && appOnly(message, ctx))
	);
	const btn =
		'h-auto min-h-8 max-w-full whitespace-normal px-2.5 py-1 text-center max-[640px]:min-h-[max(44px,2rem)]';

	async function go(run: () => Promise<void> | void) {
		if (!officeRunning || busy) return;
		busy = true;
		error = undefined;
		try {
			await run();
			handingBack = false;
			reason = '';
		} catch (e) {
			error = (e as { message?: string }).message ?? 'That did not go through.';
		} finally {
			busy = false;
		}
	}
	async function openHandBack() {
		handingBack = !handingBack;
		if (handingBack) {
			await tick();
			reasonBox?.focus();
		}
	}
</script>

<div
	data-ask-id={message.id}
	class="rounded-lg border {closed
		? 'border-line bg-tint/[0.03]'
		: 'border-brand/30 bg-brand/5'} p-4 max-[640px]:px-[8px] max-[640px]:py-3"
	data-testid="handoff-card"
	data-answered={closed ? 'true' : 'false'}
>
	<p class="text-xs/5 font-semibold text-brand-text-strong">
		<HeroIcon variant="micro" name="arrows-right-left" />
		Hand-off: {nameOf(ctx, message.from)} → {whoOf(ctx, message.addressed_to ?? '')}
		{#if where}<span class="font-normal text-fg-muted">{' · in ' + where}</span>{/if}
	</p>
	<div class="mt-2">
		<Rich body={expanded ? message.body : long.shown} />
		{#if long.hidden > 0}<button
				type="button"
				class="mt-1 text-sm/6 font-semibold text-brand-text hover:text-brand-text-strong"
				aria-expanded={expanded}
				onclick={() => (expanded = !expanded)}
				>{expanded ? 'Show less' : `Show more (${long.hidden} more lines)`}</button
			>{/if}
	</div>
	{#if message.linked_ref}
		<p class="mt-2 text-xs/5 text-fg-muted" data-testid="linked-ref">
			Linked to <span
				class="rounded bg-surface-overlay px-1.5 py-0.5 font-mono font-semibold text-fg-bright"
				>{message.linked_ref}</span
			>
		</p>
	{/if}

	{#if closed}
		<p
			class="mt-3 text-sm/6 {message.accepted_by ? 'text-positive-text' : 'text-fg-secondary'}"
			data-testid="answered"
		>
			{outcome}
		</p>
	{:else}
		<div class="mt-3 flex flex-wrap items-center gap-2">
			{#if canAccept}<Button
					type="button"
					size="sm"
					variant="outline"
					class={btn}
					disabled={busy || !officeRunning}
					onclick={() => go(onaccept)}>Accept</Button
				>{/if}
			{#if onreplyinthread}<Button
					type="button"
					size="sm"
					variant="ghost"
					class={btn}
					onclick={(event) => onreplyinthread?.(event.currentTarget)}>Reply…</Button
				>{/if}
			{#if canAccept && onhandback}<Button
					type="button"
					size="sm"
					variant="ghost"
					class={btn}
					disabled={busy || !officeRunning}
					aria-expanded={handingBack}
					onclick={openHandBack}>Hand back…</Button
				>{/if}
		</div>
		{#if handingBack}
			<form
				class="mt-2 flex flex-wrap items-start gap-2"
				onsubmit={(e) => (e.preventDefault(), go(() => onhandback?.(reason)))}
			>
				<label class="sr-only" for="handback-{message.id}">Why do you hand it back?</label>
				<textarea
					id="handback-{message.id}"
					bind:this={reasonBox}
					bind:value={reason}
					rows="2"
					maxlength={20000}
					disabled={busy}
					placeholder="Why do you hand it back?"
					class="min-w-0 flex-1 rounded-md bg-surface px-3 py-2 text-sm/6 text-fg outline-1 -outline-offset-1 outline-line focus-visible:outline-2 focus-visible:outline-focus max-[639px]:text-base"
				></textarea>
				<Button
					type="submit"
					size="sm"
					variant="outline"
					disabled={busy || !officeRunning || reason.trim() === ''}
					>Hand back to {nameOf(ctx, message.from)}</Button
				>
			</form>
		{/if}
	{/if}
	{#if error}<p class="mt-2 text-sm/6 text-danger" role="alert">{error}</p>{/if}
</div>
