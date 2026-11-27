<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import Plus from '@lucide/svelte/icons/plus';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import UserPlus from '@lucide/svelte/icons/user-plus';
	import type { HomeInboxState } from '$lib/modules/home/model';

	interface Props {
		isDayOne: boolean;
		inboxState?: HomeInboxState;
		sessionHandledCount?: number;
		lastReceipt?: { title: string; outcome: string; note?: string };
	}

	let { isDayOne, inboxState = 'ready', sessionHandledCount = 0, lastReceipt }: Props = $props();

	const clearedInSession = $derived(!isDayOne && inboxState === 'ready' && sessionHandledCount > 0);
</script>

<div class="flex flex-1 flex-col items-center justify-center gap-3 py-8 text-center">
	<span
		class={[
			'status-ring flex size-12 items-center justify-center rounded-full',
			inboxState === 'unavailable' && 'is-warn',
			inboxState === 'loading' && 'is-muted'
		]}
	>
		{#if inboxState === 'unavailable'}
			<AlertTriangle class="size-6" aria-hidden="true" />
		{:else if inboxState === 'loading'}
			<Clock class="size-6" aria-hidden="true" />
		{:else if isDayOne}
			<Sparkles class="size-6" aria-hidden="true" />
		{:else}
			<CheckCircle2 class="size-6" aria-hidden="true" />
		{/if}
	</span>
	<h2 class="text-base font-semibold text-[var(--exec-ink)]">
		{#if inboxState === 'loading'}
			Checking Inbox…
		{:else if inboxState === 'unavailable'}
			Inbox decisions unavailable
		{:else if clearedInSession}
			Priority spotlight cleared
		{:else if isDayOne}
			Launch your organization
		{:else}
			Nothing needs your decision
		{/if}
	</h2>
	<p class="max-w-md text-sm text-[var(--exec-muted)]">
		{#if inboxState === 'loading'}
			Checking Inbox for pending decisions.
		{:else if inboxState === 'unavailable'}
			Could not reach Inbox to confirm pending decisions.
		{:else if clearedInSession}
			Everything waiting on you is handled. Your agents have what they need to keep moving.
		{:else if isDayOne}
			Create your first project or hire an autonomous agent. Approvals, updates, and executive
			metrics will appear here automatically.
		{:else}
			When an agent needs an approval, decision, or answer, it appears right here.
		{/if}
	</p>
	{#if clearedInSession && lastReceipt}
		<div class="cleared-receipt mt-1 flex flex-col items-center gap-1" role="status">
			<span class="handled-pill inline-flex items-center gap-1.5 text-xs font-semibold">
				<CheckCircle2 class="size-3.5" aria-hidden="true" />
				<span>{lastReceipt.outcome} — {lastReceipt.title}</span>
			</span>
			{#if lastReceipt.note}
				<p class="receipt-quote max-w-md text-xs text-[var(--exec-body)]">{lastReceipt.note}</p>
			{/if}
		</div>
	{/if}
	<div class="mt-2 flex flex-wrap justify-center gap-2.5">
		{#if isDayOne}
			<a href={resolve('/projects')} class="exec-cta">
				<span class="exec-cta-fill" aria-hidden="true"></span>
				<span class="exec-cta-inner">
					<Plus class="size-4" aria-hidden="true" />
					<span>Create first project</span>
				</span>
			</a>
			<a
				href={resolve('/teams/members')}
				class="outline-action inline-flex min-h-9 items-center gap-1.5 px-3.5 text-xs font-semibold"
			>
				<UserPlus class="size-3.5" aria-hidden="true" />
				<span>Hire first agent</span>
			</a>
		{:else}
			<a href={resolve(inboxState === 'unavailable' ? '/inbox' : '/work')} class="exec-cta">
				<span class="exec-cta-fill" aria-hidden="true"></span>
				<span class="exec-cta-inner">
					<span>{inboxState === 'unavailable' ? 'Open Inbox' : 'Open Work'}</span>
					<ChevronRight class="size-4" aria-hidden="true" />
				</span>
			</a>
			<a
				href={resolve('/teams/members')}
				class="outline-action inline-flex min-h-9 items-center gap-1.5 px-3.5 text-xs font-semibold"
			>
				<span>Manage Members</span>
			</a>
		{/if}
	</div>
</div>

<style>
	.status-ring {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}
	.status-ring.is-warn {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
	}
	.status-ring.is-muted {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--exec-muted);
	}
	.handled-pill {
		padding: 4px 10px;
		border-radius: 999px;
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}
	.receipt-quote {
		margin: 0;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.outline-action {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.outline-action:hover {
		background: var(--exec-surface-hi);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.outline-action {
			min-height: 44px;
		}
	}
</style>
