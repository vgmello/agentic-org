<!--
  @component
  One Inbox item with everything the owner needs to act on it (our-org-in-the-app §6.1): who it is from, what
  it is, why, the recommendation when there is one, the actions the source allows and when it lapses.

  Reply, Approve, Deny, Choose and Dismiss all happen inline. Deny asks for its reason in place. When the
  source refuses, the card keeps its input and the item stays open, and shows the source's reason. An item the
  source offers no operation for gets Open instead. A handled item shows its outcome and no actions.

  Reuse anywhere an item is shown in full:
    <InboxItemCard {item} {now} {act} headingLevel={3} />
  `act` performs the request (the Inbox page passes one that calls the api and updates the list) and rejects with
  the source's reason when it refuses.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { eventTime, formatFull, relativeUntil } from '$lib/modules/preferences/time';
	import { KIND_WORD, type InboxItem, type InboxRequest } from '../../types';

	let {
		item,
		now,
		act,
		headingLevel = 3
	}: {
		item: InboxItem;
		now: Date;
		act: (item: InboxItem, request: InboxRequest) => Promise<void>;
		headingLevel?: 2 | 3;
	} = $props();

	const uid = $props.id();
	const FIELD =
		'block w-full rounded-md border-0 bg-surface-raised px-3 py-1.5 text-base/6 text-fg outline-1 -outline-offset-1 outline-field-border placeholder:text-fg-muted focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus aria-invalid:outline-danger-text min-[640px]:text-sm/6';

	type Pending = InboxRequest['type'] | undefined;
	let pending = $state<Pending>();
	let failure = $state<string | undefined>();
	let lastTried = $state<InboxRequest['type']>('approve');
	let denying = $state(false);
	let reason = $state('');
	let reasonInvalid = $state(false);
	let reply = $state('');
	let replyInvalid = $state(false);
	let choice = $state<string | undefined>();
	let reasonField = $state<HTMLTextAreaElement | null>(null);
	let replyField = $state<HTMLTextAreaElement | null>(null);
	let denyButton = $state<HTMLButtonElement | null>(null);

	const can = (action: string) => item.actions.includes(action as InboxItem['actions'][number]);
	const hasOperation = $derived(
		(can('approve') && can('deny')) || can('reply') || can('choose') || can('dismiss')
	);
	const H = $derived(`h${headingLevel}` as 'h2' | 'h3');
	const busy = $derived(pending !== undefined);

	async function run(request: InboxRequest) {
		pending = request.type;
		lastTried = request.type;
		failure = undefined;
		try {
			await act(item, request);
		} catch (e) {
			failure = e instanceof Error && e.message ? e.message : 'The source did not say why.';
		} finally {
			pending = undefined;
		}
	}

	async function startDeny() {
		failure = undefined;
		denying = true;
		await tick();
		reasonField?.focus();
	}
	async function cancelDeny() {
		denying = false;
		reasonInvalid = false;
		failure = undefined;
		await tick();
		denyButton?.focus();
	}
	function submitDeny() {
		if (reason.trim() === '') {
			reasonInvalid = true;
			reasonField?.focus();
			return;
		}
		reasonInvalid = false;
		void run({ type: 'deny', reason: reason.trim() });
	}
	function submitReply() {
		if (reply.trim() === '') {
			replyInvalid = true;
			replyField?.focus();
			return;
		}
		replyInvalid = false;
		void run({ type: 'reply', text: reply.trim() });
	}

	const expiry = $derived(item.expires_at ? new Date(item.expires_at) : undefined);
	const handledAt = $derived(
		item.handled ? eventTime(item.handled.at, now, accountZone.zone) : undefined
	);
	const verbs: Record<string, string> = {
		approve: 'approve this',
		deny: 'deny this',
		reply: 'send your reply',
		choose: 'record the decision',
		dismiss: 'dismiss this'
	};
	const busyWords: Record<string, string> = {
		approve: 'Approving…',
		deny: 'Denying…',
		reply: 'Sending…',
		choose: 'Recording…',
		dismiss: 'Dismissing…'
	};
</script>

<article
	aria-labelledby="{uid}-title"
	data-testid="inbox-card"
	data-item={item.id}
	data-state={item.state}
>
	<p class="text-xs/5 text-fg-muted">
		<span class="font-semibold text-fg-secondary">{KIND_WORD[item.kind]}</span> · {item.source}
	</p>
	<svelte:element
		this={H}
		id="{uid}-title"
		class="mt-1 text-base/7 font-semibold text-fg [overflow-wrap:anywhere]"
	>
		{item.title}
	</svelte:element>

	<dl
		class="mt-4 grid grid-cols-[6.5rem_minmax(0,1fr)] gap-x-4 gap-y-3 text-sm/6 max-[479px]:grid-cols-1 max-[479px]:gap-y-1"
	>
		<dt class="text-fg-muted">From</dt>
		<dd class="flex min-w-0 flex-wrap items-center gap-x-2 text-fg-body max-[479px]:mb-2">
			<Avatar name={item.who.name} size="sm" />
			<span class="font-semibold text-fg [overflow-wrap:anywhere]">{item.who.name}</span>
			<span class="text-fg-muted">
				{[item.who.kind === 'agent' ? 'Agent' : 'Person', item.team?.name, item.project?.name]
					.filter(Boolean)
					.join(' · ')}
			</span>
		</dd>
		<dt class="text-fg-muted">Why</dt>
		<dd class="text-fg-body [overflow-wrap:anywhere] max-[479px]:mb-2">{item.why}</dd>
		{#if item.recommendation}
			<dt class="text-fg-muted">Recommended</dt>
			<dd
				class="text-fg-body [overflow-wrap:anywhere] max-[479px]:mb-2"
				data-testid="recommendation"
			>
				{item.recommendation}
			</dd>
		{/if}
		{#if expiry && item.state === 'open'}
			<dt class="text-fg-muted">Expires</dt>
			<dd class="text-fg-body max-[479px]:mb-2">
				<time datetime={expiry.toISOString()} title={formatFull(expiry, accountZone.zone)}
					>{relativeUntil(expiry, now)}<span class="sr-only">
						({formatFull(expiry, accountZone.zone)})</span
					></time
				>
				<span class="text-fg-muted">· {formatFull(expiry, accountZone.zone)}</span>
			</dd>
		{/if}
	</dl>

	{#if item.state === 'handled' && item.handled}
		<div class="mt-5 border-t border-line pt-4" data-testid="handled-line">
			<p class="text-sm/6 text-fg-body">
				<span class="font-semibold text-fg">{item.handled.outcome}</span>
				{#if item.handled.by}<span class="text-fg-muted"> · by {item.handled.by}</span>{/if}
				<span class="text-fg-muted"> · </span>
				<time datetime={handledAt?.iso} title={handledAt?.full} class="text-fg-muted"
					>{handledAt?.text}<span class="sr-only"> ({handledAt?.full})</span></time
				>
			</p>
			{#if item.open}
				<a
					href={resolve(item.open.path as '/')}
					class="mt-2 inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
					>{item.open.label}</a
				>
			{/if}
		</div>
	{:else}
		<div class="mt-5 space-y-4 border-t border-line pt-4">
			{#if can('approve') && can('deny')}
				{#if denying}
					<form
						class="space-y-3"
						novalidate
						onsubmit={(event) => {
							event.preventDefault();
							submitDeny();
						}}
					>
						<div>
							<Label for="{uid}-reason" class="block text-sm/6 font-medium text-fg"
								>Reason for denying</Label
							>
							<textarea
								id="{uid}-reason"
								bind:this={reasonField}
								bind:value={reason}
								rows="3"
								aria-required="true"
								aria-invalid={reasonInvalid}
								aria-describedby="{uid}-reason-help{reasonInvalid ? ` ${uid}-reason-error` : ''}"
								oninput={() => (reasonInvalid = reasonInvalid && reason.trim() === '')}
								class="mt-2 {FIELD}"></textarea>
							<p id="{uid}-reason-help" class="mt-2 text-xs/5 text-fg-muted">
								{item.who.name} sees this reason.
							</p>
							{#if reasonInvalid}<p id="{uid}-reason-error" class="mt-2 text-xs/5 text-danger-text">
									Say why you are denying this, so {item.who.name} knows what to change.
								</p>{/if}
						</div>
						<div class="flex flex-wrap items-center gap-3">
							<Button
								type="submit"
								variant="destructive"
								disabled={busy}
								aria-busy={pending === 'deny'}
								class="min-w-28 max-[639px]:min-h-11"
								>{pending === 'deny' ? busyWords.deny : 'Deny'}</Button
							>
							<Button
								type="button"
								variant="outline"
								disabled={busy}
								onclick={cancelDeny}
								class="max-[639px]:min-h-11">Cancel</Button
							>
						</div>
					</form>
				{:else}
					<div class="flex flex-wrap items-center gap-3">
						<Button
							disabled={busy}
							aria-busy={pending === 'approve'}
							onclick={() => run({ type: 'approve' })}
							class="min-w-28 max-[639px]:min-h-11"
							>{pending === 'approve' ? busyWords.approve : 'Approve'}</Button
						>
						<Button
							bind:ref={denyButton}
							variant="outline"
							disabled={busy}
							onclick={startDeny}
							class="max-[639px]:min-h-11">Deny…</Button
						>
						{#if item.open}<a
								href={resolve(item.open.path as '/')}
								class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
								>{item.open.label}</a
							>{/if}
					</div>
				{/if}
			{/if}

			{#if can('reply')}
				<form
					class="space-y-3"
					novalidate
					onsubmit={(event) => {
						event.preventDefault();
						submitReply();
					}}
				>
					<div>
						<Label for="{uid}-reply" class="block text-sm/6 font-medium text-fg">Your reply</Label>
						<textarea
							id="{uid}-reply"
							bind:this={replyField}
							bind:value={reply}
							rows="3"
							aria-required="true"
							aria-invalid={replyInvalid}
							aria-describedby="{uid}-reply-help{replyInvalid ? ` ${uid}-reply-error` : ''}"
							oninput={() => (replyInvalid = replyInvalid && reply.trim() === '')}
							class="mt-2 {FIELD}"></textarea>
						<p id="{uid}-reply-help" class="mt-2 text-xs/5 text-fg-muted">
							Sent to {item.who.name} as the answer to this question.
						</p>
						{#if replyInvalid}<p id="{uid}-reply-error" class="mt-2 text-xs/5 text-danger-text">
								Write your reply before sending it.
							</p>{/if}
					</div>
					<div class="flex flex-wrap items-center gap-3">
						<Button
							type="submit"
							disabled={busy}
							aria-busy={pending === 'reply'}
							class="min-w-28 max-[639px]:min-h-11"
							>{pending === 'reply' ? busyWords.reply : 'Send reply'}</Button
						>
						{#if item.open}<a
								href={resolve(item.open.path as '/')}
								class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
								>{item.open.label}</a
							>{/if}
					</div>
				</form>
			{/if}

			{#if can('choose') && item.choices}
				<form
					class="space-y-3"
					novalidate
					onsubmit={(event) => {
						event.preventDefault();
						if (choice) void run({ type: 'choose', choice });
					}}
				>
					<fieldset>
						<legend class="text-sm/6 font-medium text-fg">Decide</legend>
						<div class="mt-2 divide-y divide-line-subtle border-y border-line">
							{#each item.choices as option (option.id)}
								<label
									class="flex min-h-11 cursor-pointer items-center gap-x-3 px-1 py-2 text-sm/6 text-fg-body hover:bg-surface-raised has-focus-visible:outline-2 has-focus-visible:-outline-offset-2 has-focus-visible:outline-focus"
								>
									<input
										type="radio"
										name="{uid}-choice"
										value={option.id}
										checked={choice === option.id}
										onchange={() => (choice = option.id)}
										class="size-4 shrink-0 accent-primary"
									/>
									<span class="min-w-0 [overflow-wrap:anywhere]">{option.label}</span>
								</label>
							{/each}
						</div>
					</fieldset>
					<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
						<Button
							type="submit"
							disabled={busy || !choice}
							aria-busy={pending === 'choose'}
							aria-describedby="{uid}-choose-help"
							class="min-w-36 max-[639px]:min-h-11"
							>{pending === 'choose' ? busyWords.choose : 'Record decision'}</Button
						>
						<p id="{uid}-choose-help" class="text-xs/5 text-fg-muted">
							{choice
								? 'This is recorded for everyone who needs it.'
								: 'Choose an option to record the decision.'}
						</p>
					</div>
				</form>
			{/if}

			{#if !hasOperation || can('dismiss')}
				<div class="flex flex-wrap items-center gap-3">
					{#if item.open}
						<a
							href={resolve(item.open.path as '/')}
							class="inline-flex h-9 items-center rounded-md border border-border bg-background px-3 text-sm font-medium text-fg hover:bg-muted focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
							>{item.open.label}</a
						>
					{/if}
					{#if can('dismiss')}
						<Button
							variant="outline"
							disabled={busy}
							aria-busy={pending === 'dismiss'}
							onclick={() => run({ type: 'dismiss' })}
							class="min-w-24 max-[639px]:min-h-11"
							>{pending === 'dismiss' ? busyWords.dismiss : 'Dismiss'}</Button
						>
					{/if}
				</div>
				{#if !hasOperation && !item.open}
					<p class="text-sm/6 text-fg-muted">This clears when its source is resolved.</p>
				{/if}
			{/if}

			{#if failure}
				<p role="alert" class="text-sm/6 text-danger-text" data-testid="action-failure">
					Couldn't {verbs[lastTried] ?? 'do that'}. {failure} The item is still open.
				</p>
			{/if}
		</div>
	{/if}
</article>
