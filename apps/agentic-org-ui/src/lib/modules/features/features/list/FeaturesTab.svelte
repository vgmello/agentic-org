<!--
  @component
  Project Features tab (`/projects/[id]/features`, features-ux §2, FTR-6, FTR-10, FTR-13, FTR-14):
  Groups features by lifecycle (In progress, Planned, Live collapsed by default, Retired filter-selected),
  shows F- reference (or Pending number), owner, derived task progress excluding cancelled tasks,
  origin links, search by ref/title/slug, and New feature action.
-->
<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import { resolve } from '$app/paths';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import AlertTriangleIcon from '@lucide/svelte/icons/alert-triangle';
	import Loaded from '$lib/components/Loaded.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { gutters, inputClass, mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { projectFrame } from '$lib/modules/projects';
	import { featuresApi } from '../../api';
	import {
		FEATURE_STATUS_TONE,
		FEATURE_STATUS_WORD,
		defaultStatusFilters,
		featureHref,
		featureProgressStats,
		featurePublicSlug,
		featureRef,
		groupFeatures,
		matchesFeatureFilter
	} from '../../present';
	import type { FeatureRow, FeatureStatus, ProjectFeaturesOverview } from '../../types';
	import NewFeatureDialog from '../dialogs/NewFeatureDialog.svelte';

	const frame = projectFrame();
	const uid = $props.id();

	const featuresRead = new Resource<ProjectFeaturesOverview>(() =>
		featuresApi.list(frame.organizationId, frame.projectId)
	);

	const selectedStatuses = new SvelteSet<FeatureStatus>(defaultStatusFilters());
	let query = $state('');
	let liveExpanded = $state(false);
	let newOpen = $state(false);

	const STATUS_CHIPS: FeatureStatus[] = ['planned', 'in_progress', 'live', 'retired'];

	const overview = $derived(featuresRead.data);
	const stale = $derived(!!overview?.stale || !!frame.project.stale);
	const canEdit = $derived(!!overview?.canEdit && !stale && !frame.project.archived);

	const filtered = $derived.by(() => {
		if (!overview) return [];
		return overview.features.filter((f) =>
			matchesFeatureFilter(f, { statuses: selectedStatuses, query })
		);
	});

	const groups = $derived(groupFeatures(filtered));

	function toggleStatus(status: FeatureStatus) {
		if (selectedStatuses.has(status)) {
			if (selectedStatuses.size > 1) selectedStatuses.delete(status);
		} else {
			selectedStatuses.add(status);
		}
	}
</script>

{#snippet featureRow(f: FeatureRow)}
	{@const stats = featureProgressStats(f.progress)}
	<li
		class="relative flex flex-wrap items-start justify-between gap-x-6 gap-y-2 py-4 hover:bg-tint/[0.03] {gutters}"
		data-testid="feature-row"
		data-feature-id={f.id}
	>
		<div class="min-w-0 flex-1 basis-64">
			<div class="flex flex-wrap items-baseline gap-x-2 gap-y-1">
				<span class="font-mono text-xs/5 font-medium text-fg-muted">{featureRef(f)}</span>
				<a
					href={resolve(featureHref(frame.projectId, f.id) as '/')}
					class="rounded-sm text-sm/6 font-semibold text-fg hover:text-brand-text focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
				>
					{f.title}
				</a>
				<StatusPill
					label={FEATURE_STATUS_WORD[f.status]}
					class={tone[FEATURE_STATUS_TONE[f.status]]}
				/>
				{#if f.pendingPublication}
					<StatusPill label="Pending publication" class={tone.owner} />
				{/if}
			</div>

			<p class="mt-1 flex flex-wrap gap-x-4 gap-y-0.5 text-xs/5 text-fg-muted">
				<span>Owner: {f.owner?.name ?? 'Unassigned'}</span>
				<span class="font-mono">{featurePublicSlug(f)}</span>
				{#each f.origins as origin (origin.id)}
					<span>
						{#if origin.unresolved}
							Origin: <span class="font-mono">{origin.id}</span> (unresolved)
						{:else}
							From <a
								href={resolve(`/work/${encodeURIComponent(origin.ref)}` as '/')}
								class="text-link hover:underline"
							>
								{origin.ref}
								{origin.title}
							</a>
						{/if}
					</span>
				{/each}
			</p>

			{#if f.problem}
				<p class="mt-1.5 inline-flex items-center gap-1.5 text-xs/5 text-warning-text-strong">
					<AlertTriangleIcon class="size-3.5 shrink-0" aria-hidden="true" />
					{f.problem.kind === 'malformed_metadata'
						? `Details problem (${f.problem.field}): ${f.problem.message}`
						: `Canonical file missing at ${f.problem.expectedPath}`}
				</p>
			{/if}
		</div>

		<!-- Derived task progress: words + accessible bar (FTR-10) -->
		<div
			class="flex w-52 shrink-0 flex-col gap-1 max-[639px]:w-full"
			data-testid="feature-progress"
		>
			<div class="flex items-baseline justify-between gap-2 text-xs/5">
				<span class="text-fg-secondary tabular-nums">{stats.label}</span>
				{#if stats.eligible > 0}
					<span class="font-mono text-fg-muted tabular-nums">{stats.percent}%</span>
				{/if}
			</div>
			<div
				role="progressbar"
				aria-label="{f.title} task progress"
				aria-valuemin={0}
				aria-valuemax={100}
				aria-valuenow={stats.percent}
				class="h-1.5 w-full overflow-hidden rounded-full bg-surface-overlay"
			>
				<div class="h-full rounded-full bg-primary" style:width="{stats.percent}%"></div>
			</div>
			{#if stats.cancelledNote}
				<p class="text-xs/5 text-fg-muted">{stats.cancelledNote}</p>
			{/if}
		</div>
	</li>
{/snippet}

<Loaded
	resource={featuresRead}
	label="the project features"
	error="Couldn't load the project features"
	errorLevel={4}
	rows={4}
>
	{#snippet children(data)}
		<div class="flex flex-col">
			<!-- Toolbar & Filters -->
			<div
				class="flex flex-wrap items-end justify-between gap-3 border-b border-line-subtle py-4 {gutters}"
			>
				<div class="flex flex-wrap items-end gap-3">
					<fieldset class="flex flex-wrap items-center gap-1.5">
						<legend class="sr-only">Filter features by status</legend>
						{#each STATUS_CHIPS as status (status)}
							{@const active = selectedStatuses.has(status)}
							<button
								type="button"
								aria-pressed={active}
								onclick={() => toggleStatus(status)}
								class="rounded-md px-2.5 py-1.5 text-xs/5 font-semibold ring-1 ring-inset focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {active
									? 'bg-surface-overlay text-fg ring-line-strong'
									: 'bg-surface-raised text-fg-muted ring-line hover:text-fg'}"
							>
								{FEATURE_STATUS_WORD[status]}
							</button>
						{/each}
					</fieldset>

					<div class="min-w-48 max-[639px]:w-full">
						<Label for="{uid}-search" class="sr-only">Search features</Label>
						<Input
							id="{uid}-search"
							type="search"
							placeholder="Search F- reference, title, or slug"
							bind:value={query}
							class={inputClass}
						/>
					</div>
				</div>

				{#if canEdit}
					<Button
						type="button"
						size="sm"
						class={mobileButton}
						onclick={() => (newOpen = true)}
						data-testid="new-feature-btn"
					>
						<PlusIcon data-icon="inline-start" />
						New feature
					</Button>
				{/if}
			</div>

			{#if data.features.length === 0}
				<EmptyState
					level={4}
					title="No features defined yet"
					description="Features are the project's capability map: each defines one capability in organization Git and links its tasks, designs, decisions, and releases."
				>
					{#snippet action()}
						{#if canEdit}
							<Button type="button" class={mobileButton} onclick={() => (newOpen = true)}>
								New feature
							</Button>
						{/if}
					{/snippet}
				</EmptyState>
			{:else if groups.length === 0}
				<div class="py-12 {gutters}" role="status">
					<p class="text-sm/6 font-semibold text-fg">No features match these filters</p>
					<p class="mt-1 text-sm/6 text-fg-muted">
						Try another reference, title, or slug, or include Retired features in the filter above.
					</p>
				</div>
			{:else}
				{#each groups as group (group.status)}
					{#if group.status === 'live'}
						<!-- Live is collapsed by default (FTR-10) -->
						<section class="border-b border-line-subtle py-4" data-testid="feature-group-live">
							<div class={gutters}>
								<button
									type="button"
									aria-expanded={liveExpanded}
									aria-controls="{uid}-live-list"
									onclick={() => (liveExpanded = !liveExpanded)}
									class="inline-flex min-h-9 items-center gap-2 rounded-sm text-xs/5 font-semibold tracking-wide text-fg-muted uppercase hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
								>
									{#if liveExpanded}
										<ChevronDownIcon class="size-4" aria-hidden="true" />
									{:else}
										<ChevronRightIcon class="size-4" aria-hidden="true" />
									{/if}
									{group.label} · {group.rows.length}
								</button>
							</div>
							{#if liveExpanded}
								<ul
									id="{uid}-live-list"
									role="list"
									class="mt-2 divide-y divide-line-subtle border-t border-line-subtle"
								>
									{#each group.rows as feat (feat.id)}
										{@render featureRow(feat)}
									{/each}
								</ul>
							{/if}
						</section>
					{:else}
						<section
							aria-labelledby="{uid}-group-{group.status}"
							class="border-b border-line-subtle pt-5"
							data-testid="feature-group-{group.status}"
						>
							<div class="pb-2 {gutters}">
								<h4
									id="{uid}-group-{group.status}"
									class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase"
								>
									{group.label} · {group.rows.length}
								</h4>
							</div>
							<ul role="list" class="divide-y divide-line-subtle">
								{#each group.rows as feat (feat.id)}
									{@render featureRow(feat)}
								{/each}
							</ul>
						</section>
					{/if}
				{/each}
			{/if}

			{#if stale}
				<div class="py-4 {gutters}">
					<Callout>
						<p class="text-sm/6">
							HQ can't be reached, so feature changes are paused. What's shown is the last copy.
						</p>
					</Callout>
				</div>
			{/if}
		</div>

		<NewFeatureDialog
			bind:open={newOpen}
			organizationId={frame.organizationId}
			projectId={frame.projectId}
			projectSlug={data.projectSlug}
			owners={data.owners}
			oncreated={() => void featuresRead.reload()}
		/>
	{/snippet}
</Loaded>
