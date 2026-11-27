<!--
  @component
  History & Compare panel for DocumentPage: issued versions, snapshot diff selector, and Git revisions.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { Label } from '$lib/components/ui/label';
	import { gutters, selectClass } from '$lib/components/classes';
	import { docHref } from '../../present';
	import type { ProjectDocumentDetail } from '../../types';
	import DocDiffView from '../../components/DocDiffView.svelte';
	import { buildCompareSnapshots } from './document-draft-helpers';

	let {
		uid,
		projectId,
		doc,
		formatWhen
	}: {
		uid: string;
		projectId: string;
		doc: ProjectDocumentDetail;
		formatWhen: (iso: string) => string;
	} = $props();

	const snapshots = $derived(buildCompareSnapshots(doc));
	let compareLeft = $derived(snapshots[1]?.key ?? '');
	let compareRight = $derived(snapshots[0]?.key ?? '');
	const leftSnap = $derived(snapshots.find((s) => s.key === compareLeft) ?? snapshots[1]);
	const rightSnap = $derived(snapshots.find((s) => s.key === compareRight) ?? snapshots[0]);
</script>

<div class="flex flex-col gap-6 py-6 {gutters}" data-testid="doc-history-panel">
	{#if doc.versions.length > 0}
		<section aria-labelledby="{uid}-versions-h">
			<h4 id="{uid}-versions-h" class="text-sm/6 font-semibold text-fg">
				Issued versions ({doc.versions.length})
			</h4>
			<ul
				role="list"
				class="mt-2 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
			>
				{#each [...doc.versions].toReversed() as ver (ver.version)}
					<li class="px-4 py-3 text-sm/6" data-testid="issued-version-row">
						<div class="flex flex-wrap items-baseline justify-between gap-2">
							<div class="flex flex-wrap items-baseline gap-2">
								<span class="font-mono font-semibold text-fg">v{ver.version}</span>
								<span class="text-fg-secondary">{ver.title}</span>
								<span class="text-xs/5 text-fg-muted">rev {ver.revision}</span>
							</div>
							<span class="text-xs/5 text-fg-muted">
								Issued <time datetime={ver.issuedAt}>{formatWhen(ver.issuedAt)}</time>
								by {ver.issuedBy.name}
							</span>
						</div>
						{#if ver.note}
							<p class="mt-1 text-xs/5 text-fg-body">{ver.note}</p>
						{/if}
						{#if ver.derivedFrom.length > 0}
							<p class="mt-1 text-xs/5 text-fg-muted">
								Derived from:
								{#each ver.derivedFrom as src, idx (`${src.documentId}-${src.version}`)}
									{#if idx > 0},
									{/if}
									<a
										href={resolve(docHref(projectId, src.documentId) as '/')}
										class="text-link hover:underline"
									>
										{src.key ? `${src.key} ` : ''}{src.title} v{src.version}
									</a>
								{/each}
							</p>
						{/if}
					</li>
				{/each}
			</ul>
		</section>
	{/if}

	{#if snapshots.length >= 2}
		<section aria-labelledby="{uid}-compare-h" class="flex flex-col gap-3">
			<h4 id="{uid}-compare-h" class="text-sm/6 font-semibold text-fg">Compare</h4>
			<div class="grid grid-cols-1 gap-3 min-[640px]:grid-cols-2">
				<div>
					<Label for="{uid}-cmp-left" class="mb-1 block text-xs/5 font-medium text-fg-secondary">
						From (older)
					</Label>
					<select id="{uid}-cmp-left" bind:value={compareLeft} class={selectClass}>
						{#each snapshots as snap (snap.key)}
							<option value={snap.key}>{snap.label}</option>
						{/each}
					</select>
				</div>
				<div>
					<Label for="{uid}-cmp-right" class="mb-1 block text-xs/5 font-medium text-fg-secondary">
						To (newer)
					</Label>
					<select id="{uid}-cmp-right" bind:value={compareRight} class={selectClass}>
						{#each snapshots as snap (snap.key)}
							<option value={snap.key}>{snap.label}</option>
						{/each}
					</select>
				</div>
			</div>
			{#if leftSnap && rightSnap}
				<DocDiffView
					before={leftSnap.body}
					after={rightSnap.body}
					label="{leftSnap.label} → {rightSnap.label}"
				/>
			{/if}
		</section>
	{/if}

	<section aria-labelledby="{uid}-revs-h">
		<h4 id="{uid}-revs-h" class="text-sm/6 font-semibold text-fg">
			Git revision history ({doc.history.length})
		</h4>
		<ul
			role="list"
			class="mt-2 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
		>
			{#each [...doc.history].toReversed() as entry (entry.revision)}
				<li class="flex flex-wrap items-baseline justify-between gap-2 px-4 py-3 text-sm/6">
					<div>
						<span class="font-mono text-xs/5 font-semibold text-fg">rev {entry.revision}</span>
						<span class="ml-2 text-fg-body">{entry.summary}</span>
					</div>
					<span class="text-xs/5 text-fg-muted">
						{entry.by.name} · <time datetime={entry.at}>{formatWhen(entry.at)}</time>
					</span>
				</li>
			{/each}
		</ul>
	</section>
</div>
