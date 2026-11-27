<script lang="ts">
	import type { ReleaseMilestone } from '$lib/modules/releases/types';
	import ActivityPortfolioRail from './ActivityPortfolioRail.svelte';
	import ActivityProjectRail from './ActivityProjectRail.svelte';
	import type { ActivityCommandCenterModel, FeedPhase, LineEventEntry } from './assembly-model';

	let {
		model,
		loading = false,
		unavailableReason = undefined,
		embedded = false,
		projectScope = false,
		inboxPhase = 'ready',
		zone = 'UTC',
		nowMs = Date.now(),
		extraLineEvents = [],
		releaseMilestones = []
	}: {
		model: ActivityCommandCenterModel;
		loading?: boolean;
		unavailableReason?: string;
		embedded?: boolean;
		projectScope?: boolean;
		inboxPhase?: FeedPhase;
		zone?: string;
		nowMs?: number;
		extraLineEvents?: readonly LineEventEntry[];
		releaseMilestones?: readonly ReleaseMilestone[];
	} = $props();
</script>

{#snippet railContent()}
	{#if projectScope}
		<ActivityProjectRail
			{model}
			{loading}
			{unavailableReason}
			{inboxPhase}
			{zone}
			{nowMs}
			{extraLineEvents}
			{releaseMilestones}
		/>
	{:else}
		<ActivityPortfolioRail {model} {loading} {unavailableReason} {embedded} />
	{/if}
{/snippet}

{#if embedded}
	<div class={['ops-rail activity-rail is-embedded', projectScope && 'project-release-rail']}>
		{@render railContent()}
	</div>
{:else}
	<aside
		class={['ops-rail activity-rail', projectScope && 'project-release-rail']}
		aria-label="Operational context"
	>
		{@render railContent()}
	</aside>
{/if}

<style>
	.ops-rail {
		min-width: 0;
		max-width: 100%;
		border: 1px solid var(--line);
		border-radius: var(--exec-inset-radius, 12px);
		background-color: var(--exec-surface);
		background-image: var(--exec-panel-sheen);
		-webkit-backdrop-filter: var(--exec-blur);
		backdrop-filter: var(--exec-blur);
		padding: 4px 16px;
	}

	.ops-rail.is-embedded {
		border: 0;
		border-radius: 0;
		background-color: transparent;
		background-image: none;
		-webkit-backdrop-filter: none;
		backdrop-filter: none;
		padding: 0 var(--activity-panel-padding, 16px) 24px;
	}

	@media (min-width: 640px) and (max-width: 1249px) {
		.ops-rail:not(.is-embedded) {
			display: grid;
			grid-template-columns: repeat(2, minmax(0, 1fr));
			gap: 16px;
			padding: 16px;
		}
		.ops-rail:not(.is-embedded) :global(.ops-section) {
			padding: 0;
		}
		.ops-rail:not(.is-embedded) :global(.ops-section + .ops-section) {
			border-top: 0;
		}
	}

	@media (min-width: 960px) and (max-width: 1249px) {
		.ops-rail:not(.is-embedded) {
			grid-template-columns: repeat(4, minmax(0, 1fr));
			gap: 14px;
		}
	}
</style>
