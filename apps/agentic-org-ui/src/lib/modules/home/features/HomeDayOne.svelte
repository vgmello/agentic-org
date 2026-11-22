<script lang="ts">
	import { resolve } from '$app/paths';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';

	interface Props {
		isDayOne: boolean;
	}

	let { isDayOne }: Props = $props();
</script>

<div class="flex flex-1 flex-col items-center justify-center gap-3 py-8 text-center">
	<span class="status-ring flex size-12 items-center justify-center rounded-full">
		<CheckCircle2 class="size-6" aria-hidden="true" />
	</span>
	<h2 class="text-base font-semibold text-[var(--exec-ink)]">
		{isDayOne ? 'Launch your organization' : 'Nothing waiting on you'}
	</h2>
	<p class="max-w-md text-sm text-[var(--exec-muted)]">
		{isDayOne
			? 'Your HQ office is online. Add your first agents, create a project, or open Work to start autonomous delivery.'
			: 'Every decision is cleared. Your agents and teams are continuing work autonomously.'}
	</p>
	<div class="mt-2 flex flex-wrap justify-center gap-2.5">
		<a href={resolve('/work')} class="exec-cta">
			<span class="exec-cta-fill" aria-hidden="true"></span>
			<span class="exec-cta-inner">
				<span>Open Work</span>
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
