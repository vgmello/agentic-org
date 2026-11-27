<!--
  @component
  "By kind" grouped layout for DocsTab: Brief pinned, kind groups, Known limitations per feature, and Older deprecated.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { gutters } from '$lib/components/classes';
	import type { KnownLimitationItem, ProjectDocsOverview, ProjectDocumentRow } from '../../types';
	import DocsListRow from './DocsListRow.svelte';

	let {
		uid,
		projectId,
		projectName,
		kinds,
		brief,
		groups,
		deprecated,
		knownLimitations,
		hasFilters,
		olderOpen = $bindable(false),
		formatWhen
	}: {
		uid: string;
		projectId: string;
		projectName: string;
		kinds: ProjectDocsOverview['kinds'] | undefined;
		brief: ProjectDocumentRow | null;
		groups: { kind: string; label: string; docs: ProjectDocumentRow[] }[];
		deprecated: ProjectDocumentRow[];
		knownLimitations: readonly KnownLimitationItem[];
		hasFilters: boolean;
		olderOpen: boolean;
		formatWhen: (iso: string) => string;
	} = $props();
</script>

{#if brief}
	<section aria-labelledby="{uid}-brief-group" class="border-b border-line-subtle pt-5">
		<div class="flex items-baseline justify-between gap-2 pb-2 {gutters}">
			<h4
				id="{uid}-brief-group"
				class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase"
			>
				Brief (pinned)
			</h4>
		</div>
		<ul role="list" class="divide-y divide-line-subtle">
			<DocsListRow doc={brief} {projectId} {projectName} {kinds} {formatWhen} />
		</ul>
	</section>
{/if}

{#each groups as group (group.kind)}
	<section
		aria-labelledby="{uid}-group-{group.kind}"
		class="border-b border-line-subtle pt-5"
		data-testid="doc-kind-group"
		data-kind={group.kind}
	>
		<div class="flex items-baseline justify-between gap-2 pb-2 {gutters}">
			<h4
				id="{uid}-group-{group.kind}"
				class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase"
			>
				{group.label} · {group.docs.length}
			</h4>
		</div>
		<ul role="list" class="divide-y divide-line-subtle">
			{#each group.docs as doc (doc.id)}
				<DocsListRow {doc} {projectId} {projectName} {kinds} {formatWhen} />
			{/each}
		</ul>
	</section>
{/each}

{#if knownLimitations.length > 0 && !hasFilters}
	<section
		aria-labelledby="{uid}-limitations"
		class="border-b border-line-subtle py-5 {gutters}"
		data-testid="known-limitations-section"
	>
		<h4
			id="{uid}-limitations"
			class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase"
		>
			Known limitations · {knownLimitations.length}
		</h4>
		<p class="mt-1 text-xs/5 text-fg-muted">
			Kept in each feature's canonical definition file; no separate limitation record is created.
		</p>
		<ul
			role="list"
			class="mt-3 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
		>
			{#each knownLimitations as lim (`${lim.featureId}-${lim.text}`)}
				<li class="px-4 py-3 text-sm/6">
					<div class="flex flex-wrap items-baseline justify-between gap-2">
						<a
							href={resolve(
								`/projects/${encodeURIComponent(projectId)}/features/${encodeURIComponent(lim.featureId)}` as '/'
							)}
							class="font-semibold text-link hover:underline"
						>
							<span class="font-mono text-xs/5">{lim.featureRef}</span>
							{lim.featureTitle}
						</a>
						<span class="text-xs/5 text-fg-muted">{lim.featureStatus}</span>
					</div>
					<p class="mt-1 text-fg-body [overflow-wrap:anywhere]">{lim.text}</p>
				</li>
			{/each}
		</ul>
	</section>
{/if}

{#if deprecated.length > 0}
	<section class="border-b border-line-subtle py-4" data-testid="older-deprecated">
		<div class={gutters}>
			<button
				type="button"
				aria-expanded={olderOpen}
				aria-controls="{uid}-older-list"
				onclick={() => (olderOpen = !olderOpen)}
				class="inline-flex min-h-9 items-center gap-2 rounded-sm text-xs/5 font-semibold tracking-wide text-fg-muted uppercase hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
			>
				{#if olderOpen}
					<ChevronDownIcon class="size-4" aria-hidden="true" />
				{:else}
					<ChevronRightIcon class="size-4" aria-hidden="true" />
				{/if}
				Older · {deprecated.length} (deprecated)
			</button>
		</div>
		{#if olderOpen}
			<ul
				id="{uid}-older-list"
				role="list"
				class="mt-2 divide-y divide-line-subtle border-t border-line-subtle"
			>
				{#each deprecated as doc (doc.id)}
					<DocsListRow {doc} {projectId} {projectName} {kinds} {formatWhen} />
				{/each}
			</ul>
		{/if}
	</section>
{/if}
