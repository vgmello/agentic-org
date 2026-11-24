<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import type { HomeInboxState } from '$lib/home/model';

	interface Props {
		isDayOne: boolean;
		inboxState?: HomeInboxState;
	}

	let { isDayOne, inboxState = 'ready' }: Props = $props();
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
		{:else}
			<CheckCircle2 class="size-6" aria-hidden="true" />
		{/if}
	</span>
	<h2 class="text-base font-semibold text-[var(--exec-ink)]">
		{#if inboxState === 'loading'}
			Checking Inbox…
		{:else if inboxState === 'unavailable'}
			Inbox decisions unavailable
		{:else if isDayOne}
			Launch your organization
		{:else}
			Nothing waiting on you
		{/if}
	</h2>
	<p class="max-w-md text-sm text-[var(--exec-muted)]">
		{#if inboxState === 'loading'}
			Checking Inbox for pending decisions.
		{:else if inboxState === 'unavailable'}
			Could not reach Inbox to confirm pending decisions.
		{:else if isDayOne}
			Your HQ office is online. Add your first agents, create a project, or open Work to start
			autonomous delivery.
		{:else}
			Every decision is cleared. Your agents and teams are continuing work autonomously.
		{/if}
	</p>
	<div class="mt-2 flex flex-wrap justify-center gap-2.5">
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
