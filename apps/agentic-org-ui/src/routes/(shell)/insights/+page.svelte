<script lang="ts">
	import { resolve } from '$app/paths';
	import Activity from '@lucide/svelte/icons/activity';
	import BarChart3 from '@lucide/svelte/icons/bar-chart-3';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import DollarSign from '@lucide/svelte/icons/dollar-sign';
	import Network from '@lucide/svelte/icons/network';
	import Users from '@lucide/svelte/icons/users';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { pageTitle } from '$lib/nav';

	let { data } = $props();

	const destinations = [
		{
			title: 'Costs & budgets',
			description:
				'Organization spend by project, consumer, and work type, plus agent budget meters.',
			href: '/settings/costs',
			icon: DollarSign
		},
		{
			title: 'Activity feed',
			description:
				'Chronological updates across work items, agent runs, conversations, and decisions.',
			href: '/activity',
			icon: Activity
		},
		{
			title: 'Teams & members',
			description: 'Team structure, people and agent directory, and live presence.',
			href: '/teams',
			icon: Users
		},
		{
			title: 'Org chart',
			description: 'Explicit reporting hierarchy and organization ownership.',
			href: '/teams/org-chart',
			icon: Network
		}
	] as const;
</script>

<svelte:head><title>{pageTitle(undefined, 'Insights')}</title></svelte:head>

<PageHeader
	title="Insights"
	description="Trends, cycle-time analysis, and spend forecasts for {data.connection.organization
		.name}."
/>

<div class="flex flex-col gap-6 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
	<section
		aria-labelledby="insights-status-heading"
		class="insights-banner flex max-w-4xl flex-col gap-2 p-5 sm:p-6"
	>
		<div class="flex items-center gap-2.5">
			<span
				class="insights-icon-badge flex size-9 shrink-0 items-center justify-center rounded-lg"
				aria-hidden="true"
			>
				<BarChart3 class="size-4" />
			</span>
			<h3 id="insights-status-heading" class="text-base/7 font-semibold text-fg">
				Insights isn't built yet
			</h3>
		</div>
		<p class="max-w-2xl text-sm/6 text-fg-muted">
			Historical trend charts and automated org-wide insights are coming in this release. In the
			meantime, live operational breakdowns are available directly in the views below.
		</p>
	</section>

	<section aria-labelledby="insights-links-heading" class="max-w-4xl">
		<h3
			id="insights-links-heading"
			class="text-xs/5 font-semibold tracking-wider text-fg-subtle uppercase"
		>
			Operational views available now
		</h3>
		<ul role="list" class="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
			{#each destinations as dest (dest.href)}
				{@const Icon = dest.icon}
				<li>
					<a
						href={resolve(dest.href as '/')}
						class="exec-card group flex min-h-11 items-start gap-3.5 p-4 transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					>
						<span
							class="insights-icon-badge mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-lg"
							aria-hidden="true"
						>
							<Icon class="size-4" />
						</span>
						<span class="min-w-0 flex-1">
							<span class="block text-sm/6 font-semibold text-fg">{dest.title}</span>
							<span class="mt-0.5 block text-xs/5 text-fg-muted">{dest.description}</span>
						</span>
						<ChevronRight
							class="mt-1 size-4 shrink-0 text-fg-muted transition-transform group-hover:translate-x-0.5 group-hover:text-fg motion-reduce:transition-none"
							aria-hidden="true"
						/>
					</a>
				</li>
			{/each}
		</ul>
	</section>
</div>

<style>
	.insights-banner {
		background: var(--exec-surface);
		-webkit-backdrop-filter: var(--exec-blur);
		backdrop-filter: var(--exec-blur);
		box-shadow: var(--exec-shadow);
		border-radius: 16px;
	}
	:global(html[data-ui-theme^='report']) .insights-banner {
		border-radius: 4px;
		box-shadow: inset 0 0 0 1px var(--report-rule);
	}
	.insights-icon-badge {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
</style>
