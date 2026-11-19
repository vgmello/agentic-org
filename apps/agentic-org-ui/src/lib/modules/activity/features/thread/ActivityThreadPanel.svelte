<!--
  @component
  The Open thread side panel (our-org-in-the-app §6): an authorized thread, read-only. Reading never grants
  authority to answer: unless the owner is a participant allowed to reply, the panel says "Reading as the
  owner. To weigh in, message {agent}". A message the owner may not see is named as hidden, not skipped.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatClock, formatDate, formatFull } from '$lib/modules/preferences/time';
	import { activityApi } from '../../api';
	import type { ActivityThread } from '../../types';

	let {
		open = $bindable(false),
		organizationId,
		threadId,
		title,
		onCloseAutoFocus
	}: {
		open: boolean;
		organizationId: string;
		threadId: string | undefined;
		/** Who the thread is with, for the panel's title. */
		title: string;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let phase = $state<'loading' | 'ready' | 'error'>('loading');
	let thread = $state.raw<ActivityThread | undefined>();
	let failure = $state<string | undefined>();
	let request = 0;

	async function load() {
		if (!threadId) return;
		const mine = ++request;
		phase = 'loading';
		try {
			const found = await activityApi.thread(organizationId, threadId);
			if (mine !== request) return;
			thread = found;
			phase = 'ready';
		} catch (e) {
			if (mine !== request) return;
			failure = e instanceof Error ? e.message : undefined;
			phase = 'error';
		}
	}
	$effect(() => {
		if (open && threadId) void load();
	});
	const now = $derived(new Date());
</script>

<Sheet bind:open>
	<SheetContent side="right" class="w-full gap-0 sm:max-w-md" {onCloseAutoFocus}>
		<SheetHeader class="border-b border-white/10 pr-14">
			<SheetTitle class="text-base/7 font-semibold [overflow-wrap:anywhere]"
				>Thread with {title}</SheetTitle
			>
			<SheetDescription class="text-sm/6" data-testid="thread-owner-line">
				{#if phase === 'ready' && thread?.ownerMayReply}
					You are in this conversation.
					<a
						href={resolve('/chat')}
						class="rounded-sm font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
						>Reply in Chat</a
					>
				{:else}
					Reading as the owner. To weigh in, message {thread?.counterpart.name ?? title}.
				{/if}
			</SheetDescription>
		</SheetHeader>
		<div class="min-h-0 flex-1 overflow-y-auto">
			{#if phase === 'loading'}
				<SkeletonRows label="the thread" rows={4} avatar />
			{:else if phase === 'error'}
				<ErrorState
					title="Couldn't load the thread"
					message={failure ?? 'The office did not answer. Try again.'}
					onretry={load}
				/>
			{:else if thread}
				{#if thread.messages.length === 0}
					<p class="px-4 py-8 text-sm/6 text-gray-400">No messages in this thread yet.</p>
				{:else}
					<ol class="divide-y divide-white/5" aria-label="Messages">
						{#each thread.messages as message (message.id)}
							{@const at = new Date(message.at)}
							<li class="flex gap-x-3 px-4 py-4">
								<Avatar name={message.from.name} size="sm" class="mt-0.5" />
								<div class="min-w-0 flex-1">
									<p class="text-sm/6">
										<span class="font-semibold text-white">{message.from.name}</span>
										<time
											datetime={at.toISOString()}
											class="ml-2 text-xs/5 text-gray-400"
											title={formatFull(at, accountZone.zone)}
											>{formatDate(at, accountZone.zone, now)}, {formatClock(
												at,
												accountZone.zone
											)}<span class="sr-only"> ({formatFull(at, accountZone.zone)})</span></time
										>
									</p>
									{#if message.body !== undefined}
										<p class="mt-1 text-sm/6 text-gray-200 [overflow-wrap:anywhere]">
											{message.body}
										</p>
									{:else}
										<p class="mt-1 text-sm/6 text-gray-400">This message isn't shared with you.</p>
									{/if}
								</div>
							</li>
						{/each}
					</ol>
				{/if}
			{/if}
		</div>
	</SheetContent>
</Sheet>
