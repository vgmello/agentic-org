<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { resolve } from '$app/paths';
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import type { DraftText } from '../../drafts/store';
	import { Button } from '$lib/components/ui/button';
	import {
		answerText,
		optionsOf,
		questionsOf,
		type ChatContext
	} from '$lib/modules/messaging/engine';
	import {
		appOnly,
		collapse,
		deadlineWords,
		nameOf,
		outcomeWords,
		requiredWords,
		waitingFor,
		whoOf
	} from '$lib/modules/messaging/model';
	import type { AskOption, ChatMessage, QuestionAnswer } from '$lib/modules/messaging/types';
	import Rich from '../../components/Rich.svelte';
	import QuestionForm from '$lib/modules/chat/features/answer-ask/QuestionForm.svelte';
	import HandoffCard from '$lib/modules/chat/features/hand-off/HandoffCard.svelte';

	/**
	 * An ask or an approval as a card (chat-ux §2, §2.1; REQ-04; CHAT-5, CHAT-6, CHAT-44). Only its addressed actor answers;
	 * its asker can withdraw it. A hand-off is drawn by `HandoffCard`. Where the card is `readOnly` (the bridge preview) an
	 * ask to the owner and every approval show **Open in Agentic Org** instead of buttons.
	 */
	let {
		message,
		ctx,
		now,
		readOnly = false,
		officeRunning = true,
		where,
		draft,
		onanswer,
		onaccept,
		onhandback,
		onwithdraw,
		onreplyinthread
	}: {
		message: ChatMessage;
		ctx: ChatContext;
		now: string;
		readOnly?: boolean;
		officeRunning?: boolean;
		/** Shown on cards outside their conversation (Mentions, search): "in #atlas". */
		where?: string;
		draft?: DraftText;
		onanswer: (answer: {
			option_id?: string;
			text?: string;
			answers?: QuestionAnswer[];
		}) => Promise<void> | void;
		onaccept: () => Promise<void> | void;
		onhandback?: (reason: string) => Promise<void> | void;
		onwithdraw?: (reason: string) => Promise<void> | void;
		onreplyinthread?: (opener: HTMLElement) => void;
	} = $props();

	let expanded = $state(false);
	let mode = $state<'reply' | 'reject' | 'withdraw' | undefined>();
	let text = $state('');
	let replyBox = $state<HTMLTextAreaElement>();
	let busy = $state(false);
	let error = $state<string | undefined>();

	const long = $derived(collapse(message.body));
	const questions = $derived(questionsOf(message));
	// One single choice (or yes/no) is answered with its buttons; anything else is a form.
	const asForm = $derived(
		!message.approval &&
			(questions.length > 1 ||
				(questions[0].question_type === 'choice' && !!questions[0].allow_multiple))
	);
	const buttons = $derived<AskOption[]>(
		message.approval
			? (message.options ?? [])
			: !asForm && questions[0].question_type !== 'free_text'
				? optionsOf(questions[0])
				: []
	);
	const defaultId = $derived(
		message.approval ? undefined : (message.default_option ?? questions[0].default_option)
	);
	const to = $derived(whoOf(ctx, message.addressed_to ?? ''));
	const deadline = $derived(deadlineWords(message, now));
	const required = $derived(requiredWords(message, ctx, now));
	const blockedHere = $derived(readOnly && appOnly(message, ctx));
	const closed = $derived(!!message.answer || !!message.withdrawn);
	const outcome = $derived(outcomeWords(message, ctx));
	// Only the addressed actor answers; the owner reading someone else's ask gains no answer (chat-ux §2.1).
	const canAnswer = $derived(message.addressed_to === ctx.actor);
	const isAsker = $derived(message.from === ctx.actor);
	const btn =
		'h-auto min-h-8 max-w-full whitespace-normal px-2.5 py-1 text-center max-[640px]:min-h-[max(44px,2rem)]';

	$effect.pre(() => {
		const current = draft;
		if (!current) return;
		text = current.read();
		mode = undefined;
	});
	$effect(() => {
		const element = replyBox;
		if (!element || !draft) return;
		const end = untrack(() => text.length);
		element.setSelectionRange(end, end);
	});
	$effect(() => {
		if (closed) draft?.write('');
	});

	async function go(run: () => Promise<void> | void) {
		if (!officeRunning || busy) return;
		const origin = document.activeElement;
		const answeringDraft = draft;
		const clearDraft = answeringDraft?.clearOnSuccess();
		busy = true;
		error = undefined;
		try {
			await run();
			clearDraft?.();
			if (answeringDraft === draft) {
				mode = undefined;
				text = '';
			}
		} catch (e) {
			error = (e as { message?: string }).message ?? 'That did not go through.';
		} finally {
			busy = false;
			await tick();
			// A delayed refusal can blur a disabled control. Restore it only if focus was lost here.
			if (
				error &&
				origin instanceof HTMLElement &&
				origin.isConnected &&
				document.activeElement === document.body
			)
				origin.focus({ preventScroll: true });
		}
	}

	const textLabel = $derived(
		mode === 'reject'
			? 'Why do you reject it?'
			: mode === 'withdraw'
				? 'Why withdraw it? (optional)'
				: 'Your reply'
	);
	const submitLabel = $derived(
		mode === 'reject' ? 'Reject' : mode === 'withdraw' ? 'Withdraw' : 'Send reply'
	);

	function submitText(event: SubmitEvent) {
		event.preventDefault();
		if (mode === 'withdraw') return void go(() => onwithdraw?.(text));
		return void go(() => onanswer(mode === 'reject' ? { option_id: 'reject', text } : { text }));
	}
</script>

{#if message.kind === 'handoff'}
	<HandoffCard
		{message}
		{ctx}
		{readOnly}
		{officeRunning}
		{where}
		{onaccept}
		{onhandback}
		{onreplyinthread}
	/>
{:else}
	<div
		data-ask-id={message.id}
		class="rounded-lg border {closed
			? 'border-line bg-tint/[0.03]'
			: 'border-warning/40 bg-warning/5'} p-4 max-[640px]:px-[8px] max-[640px]:py-3"
		data-testid="ask-card"
		data-answered={closed ? 'true' : 'false'}
	>
		<p class="text-xs/5 font-semibold text-warning-text-strong">
			<HeroIcon variant="micro" name={message.approval ? 'check-circle' : 'question-mark-circle'} />
			{message.approval ? 'Approval' : 'Ask'}: {nameOf(ctx, message.from)} asks {to}
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

		{#if !closed}
			<p class="mt-2 text-xs/5 text-fg-muted" aria-live="off">
				<span data-testid="waiting">{waitingFor(message, now)}</span>{#if message.blocks}
					· blocks
					<span
						class="rounded bg-surface-overlay px-1.5 py-0.5 font-mono font-semibold text-fg-bright"
						>{message.blocks}</span
					>{/if}
			</p>
			{#if required}<p
					class="mt-1 text-xs/5 font-semibold text-warning-text-strong"
					aria-live="off"
					data-testid="required"
				>
					{required}
				</p>{/if}
			{#if deadline}<p
					class="mt-1 text-xs/5 text-warning-text-stronger"
					aria-live="off"
					data-testid="deadline"
				>
					{deadline.text}
				</p>{/if}
		{/if}

		{#if closed}
			<p
				class="mt-3 text-sm/6 {message.answer && message.answer.by !== 'default'
					? 'text-positive-text'
					: 'text-fg-secondary'}"
				data-testid="answered"
			>
				{outcome}
			</p>
			{#if message.answer?.by === 'default'}<p
					class="text-xs/5 text-fg-muted"
					data-testid="default-note"
				>
					Nobody answered. {nameOf(ctx, message.from)} continued with the default.
				</p>{/if}
			{#if message.answer?.answers && questions.length > 1}
				<dl class="mt-2 space-y-1 text-xs/5">
					{#each questions as q (q.id)}
						<div class="flex flex-wrap gap-x-2">
							<dt class="text-fg-muted">{q.prompt}</dt>
							<dd class="text-fg-body">
								{answerText(
									q,
									message.answer.answers.find((x) => x.question_id === q.id)
								)}
							</dd>
						</div>
					{/each}
				</dl>
			{/if}
		{:else if blockedHere}
			<p
				class="mt-3 rounded-md bg-surface-raised px-3 py-2 text-sm/6 text-fg-secondary"
				data-testid="app-only"
			>
				This is answered only in Agentic Org. <a
					href={resolve('/chat')}
					class="font-semibold text-brand-text underline">Open in Agentic Org</a
				>
			</p>
		{:else if canAnswer}
			{#if asForm}
				<QuestionForm
					askId={message.id}
					{questions}
					disabled={busy || !officeRunning}
					onsubmit={(answers) => go(() => onanswer({ answers }))}
				/>
			{:else}
				<div class="mt-3 flex flex-wrap items-start gap-2">
					{#each buttons as o (o.id)}
						{@const isDefault = o.id === defaultId}
						{#if message.approval && o.id === 'reject'}
							<Button
								type="button"
								size="sm"
								class={btn}
								variant="outline"
								disabled={busy || !officeRunning}
								aria-expanded={mode === 'reject'}
								onclick={() => (mode = mode === 'reject' ? undefined : 'reject')}>{o.label}</Button
							>
						{:else}
							<Button
								type="button"
								size="sm"
								class="{btn} {o.description ? 'flex-col gap-0' : ''} {isDefault
									? 'bg-primary/15 outline-brand/60'
									: ''}"
								variant={message.approval ? 'default' : 'outline'}
								disabled={busy || !officeRunning}
								onclick={() => go(() => onanswer({ option_id: o.id }))}
							>
								<span
									>{o.label}{#if isDefault && !/default/i.test(o.label)}<span
											class="ml-1.5 text-xs/5 font-normal text-brand-text-strong">Default</span
										>{/if}</span
								>
								{#if o.description}<span class="text-xs/5 font-normal text-fg-muted"
										>{o.description}</span
									>{/if}
							</Button>
						{/if}
					{/each}
					{#if !message.approval}
						<Button
							type="button"
							size="sm"
							variant="ghost"
							class={btn}
							disabled={busy || !officeRunning}
							aria-expanded={mode === 'reply'}
							onclick={() => (mode = mode === 'reply' ? undefined : 'reply')}>Reply…</Button
						>
					{/if}
				</div>
			{/if}
		{:else}
			<p class="mt-3 text-sm/6 text-fg-secondary" data-testid="not-yours">
				Waiting for {to}'s answer.
			</p>
		{/if}
		{#if !closed && isAsker && onwithdraw && !blockedHere}
			<div class="mt-2">
				<Button
					type="button"
					size="sm"
					variant="ghost"
					class={btn}
					disabled={busy || !officeRunning}
					aria-expanded={mode === 'withdraw'}
					onclick={() => (mode = mode === 'withdraw' ? undefined : 'withdraw')}>Withdraw…</Button
				>
			</div>
		{/if}
		{#if mode && !closed}
			<form class="mt-2 flex flex-wrap items-start gap-2" onsubmit={submitText}>
				<label class="sr-only" for="ask-text-{message.id}">{textLabel}</label>
				<textarea
					id="ask-text-{message.id}"
					bind:this={replyBox}
					bind:value={text}
					oninput={(event) => {
						if (mode !== 'withdraw') draft?.write(event.currentTarget.value);
					}}
					rows="2"
					maxlength={20000}
					disabled={busy}
					placeholder={textLabel}
					class="min-w-0 flex-1 rounded-md bg-surface px-3 py-2 text-sm/6 text-fg outline-1 -outline-offset-1 outline-line focus-visible:outline-2 focus-visible:outline-focus max-[639px]:text-base"
				></textarea>
				<Button
					type="submit"
					size="sm"
					disabled={busy || !officeRunning || (mode !== 'withdraw' && text.trim() === '')}
					>{submitLabel}</Button
				>
			</form>
		{/if}
		{#if error}<p class="mt-2 text-sm/6 text-danger" role="alert">{error}</p>{/if}
	</div>
{/if}
