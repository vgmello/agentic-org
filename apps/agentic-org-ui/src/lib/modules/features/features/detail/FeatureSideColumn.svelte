<!--
  @component
  Side column for FeaturePage: Supporting knowledge, Included in release, Cost and time, and Git revision history.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { formatMicroUsd } from '../../present';
	import type { FeatureDetail } from '../../types';

	let {
		uid,
		projectId,
		feature,
		formatWhen
	}: {
		uid: string;
		projectId: string;
		feature: FeatureDetail;
		formatWhen: (iso: string) => string;
	} = $props();
</script>

<aside aria-label="Feature context" class="flex flex-col gap-5">
	<!-- Supporting designs, decisions & reports -->
	<section
		aria-labelledby="{uid}-docs-h"
		class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
		data-testid="feature-supporting-docs"
	>
		<h4 id="{uid}-docs-h" class="text-sm/6 font-semibold text-fg">
			Supporting knowledge ({feature.supportingDocs.length})
		</h4>
		{#if feature.supportingDocs.length === 0}
			<p class="text-xs/5 text-fg-muted">No supporting designs, decisions, or reports yet.</p>
		{:else}
			<ul role="list" class="flex flex-col gap-2 text-sm/6">
				{#each feature.supportingDocs as doc (doc.id)}
					<li class="flex items-baseline justify-between gap-2">
						<a
							href={resolve(
								`/projects/${encodeURIComponent(projectId)}/docs/${encodeURIComponent(doc.id)}` as '/'
							)}
							class="text-link hover:underline [overflow-wrap:anywhere]"
						>
							{#if doc.key}
								<span class="mr-1 font-mono text-xs/5">{doc.key}</span>
							{/if}
							{doc.title}
						</a>
						<span class="shrink-0 text-xs/5 text-fg-muted">{doc.kind} · {doc.status}</span>
					</li>
				{/each}
			</ul>
		{/if}
	</section>

	<!-- Included in releases (FTR-6, FTR-11) -->
	<section
		aria-labelledby="{uid}-releases-h"
		class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
		data-testid="feature-releases"
	>
		<h4 id="{uid}-releases-h" class="text-sm/6 font-semibold text-fg">
			Included in release ({feature.releases.length})
		</h4>
		{#if feature.releases.length === 0}
			<p class="text-xs/5 text-fg-muted">Not listed in any release manifest yet.</p>
		{:else}
			<ul role="list" class="flex flex-col gap-2 text-sm/6">
				{#each feature.releases as rel (rel.releaseId)}
					<li class="flex flex-col gap-0.5">
						<div class="flex items-baseline justify-between gap-2">
							<a
								href={resolve(
									`/projects/${encodeURIComponent(projectId)}/releases/${encodeURIComponent(rel.releaseId)}` as '/'
								)}
								class="font-medium text-link hover:underline"
							>
								<span class="mr-1 font-mono text-xs/5">{rel.ref}</span>
								{rel.title}
							</a>
							<span class="text-xs/5 text-fg-muted">{rel.status}</span>
						</div>
						{#if rel.productChangelogDocId}
							<a
								href={resolve(
									`/projects/${encodeURIComponent(projectId)}/docs/${encodeURIComponent(rel.productChangelogDocId)}` as '/'
								)}
								class="text-xs/5 text-link hover:underline"
							>
								{rel.productChangelogTitle ?? 'Product changelog'}
							</a>
						{/if}
					</li>
				{/each}
			</ul>
			<p class="mt-1 text-xs/5 text-fg-muted">
				Manifest inclusion does not by itself prove deployment or availability.
			</p>
		{/if}
	</section>

	<!-- Cost and time (FTR-11) -->
	<section
		aria-labelledby="{uid}-cost-h"
		class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
		data-testid="feature-cost"
	>
		<h4 id="{uid}-cost-h" class="text-sm/6 font-semibold text-fg">Cost and time</h4>
		{#if !feature.cost.allowed}
			<p class="text-xs/5 text-fg-muted">{feature.cost.reason}</p>
		{:else}
			<dl class="divide-y divide-line-subtle text-xs/5">
				<div class="flex justify-between gap-2 py-1.5">
					<dt class="text-fg-muted">Known spend</dt>
					<dd class="font-mono text-fg">
						{feature.cost.knownCostMicroUsd === null
							? 'Unavailable'
							: `${feature.cost.complete ? '' : 'At least '}${formatMicroUsd(feature.cost.knownCostMicroUsd)}`}
					</dd>
				</div>
				<div class="flex justify-between gap-2 py-1.5">
					<dt class="text-fg-muted">Known tokens</dt>
					<dd class="font-mono text-fg">
						{feature.cost.knownTokens === null
							? 'Unavailable'
							: feature.cost.knownTokens.toLocaleString('en-US')}
					</dd>
				</div>
				{#if feature.cost.leadTimeLabel}
					<div class="flex justify-between gap-2 py-1.5">
						<dt class="text-fg-muted">Lead time</dt>
						<dd class="text-fg">{feature.cost.leadTimeLabel}</dd>
					</div>
				{/if}
			</dl>
			{#if !feature.cost.complete && feature.cost.incompleteReason}
				<p class="text-xs/5 text-warning-text-strong">
					Incomplete usage: {feature.cost.incompleteReason}
				</p>
			{/if}
		{/if}
	</section>

	<!-- Git revision history -->
	<section
		aria-labelledby="{uid}-hist-h"
		class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
		data-testid="feature-history"
	>
		<h4 id="{uid}-hist-h" class="text-sm/6 font-semibold text-fg">
			Git revision history ({feature.history.length})
		</h4>
		<ul role="list" class="flex flex-col gap-2 text-xs/5">
			{#each [...feature.history].toReversed() as entry (entry.revision)}
				<li class="border-b border-line-subtle pb-2 last:border-b-0 last:pb-0">
					<p class="text-fg-body">
						<span class="font-mono font-semibold text-fg">rev {entry.revision}</span> · {entry.summary}
					</p>
					<p class="text-fg-muted">
						{entry.by.name} · <time datetime={entry.at}>{formatWhen(entry.at)}</time>
					</p>
				</li>
			{/each}
		</ul>
	</section>
</aside>
