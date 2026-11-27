<!--
  @component
  Single document row rendered inside DocsTab ("By kind" and "By section" views).
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangleIcon from '@lucide/svelte/icons/alert-triangle';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Button } from '$lib/components/ui/button';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { docHref, isTreeDocument, kindTitle, scopeLabel } from '../../present';
	import type { ProjectDocsOverview, ProjectDocumentRow } from '../../types';
	import DocLifecycleBadge from '../../components/DocLifecycleBadge.svelte';

	let {
		doc,
		projectId,
		projectName,
		kinds,
		showMove = false,
		canEdit = false,
		sectionCount = 0,
		formatWhen,
		onmove
	}: {
		doc: ProjectDocumentRow;
		projectId: string;
		projectName: string;
		kinds: ProjectDocsOverview['kinds'] | undefined;
		showMove?: boolean;
		canEdit?: boolean;
		sectionCount?: number;
		formatWhen: (iso: string) => string;
		onmove?: (doc: ProjectDocumentRow) => void;
	} = $props();
</script>

<li
	class="relative flex flex-wrap items-start justify-between gap-x-4 gap-y-2 py-4 hover:bg-tint/[0.03] {gutters}"
	data-testid="doc-row"
	data-doc-id={doc.id}
>
	<div class="min-w-0 flex-1 basis-60">
		<div class="flex flex-wrap items-baseline gap-x-2 gap-y-1">
			{#if doc.key}
				<span class="font-mono text-xs/5 font-medium text-fg-muted">{doc.key}</span>
			{/if}
			<a
				href={resolve(docHref(projectId, doc.id) as '/')}
				class="rounded-sm text-sm/6 font-semibold text-fg hover:text-brand-text focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
			>
				{doc.title}
			</a>
			<DocLifecycleBadge status={doc.status} />
			{#if doc.version !== null}
				<StatusPill label="v{doc.version}" class={tone.neutral} />
			{/if}
			{#if doc.hasUnissuedChanges}
				<StatusPill label="Unissued changes" class={tone.owner} />
			{/if}
			{#if doc.pendingPublication && doc.pendingPublication > 0}
				<StatusPill
					label="Waiting to publish · {doc.pendingPublication} {doc.pendingPublication === 1
						? 'change'
						: 'changes'}"
					class={tone.owner}
				/>
			{/if}
		</div>
		{#if doc.summary}
			<p class="mt-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">{doc.summary}</p>
		{/if}
		{#if doc.evaluationExcerpt}
			<p class="mt-1 text-xs/5 text-fg-body [overflow-wrap:anywhere]" data-testid="eval-excerpt">
				<span class="font-semibold text-fg-muted">Excerpt:</span>
				{doc.evaluationExcerpt.question} → {doc.evaluationExcerpt.verdict}
			</p>
		{/if}
		<p class="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-xs/5 text-fg-muted">
			<span>{kindTitle(doc.kind, kinds)}</span>
			<span>{scopeLabel(doc.scope, projectName)}</span>
			<span>Owner: {doc.owner?.name ?? 'Unassigned'}</span>
			<span>
				Updated <time datetime={doc.updatedAt}>{formatWhen(doc.updatedAt)}</time>
			</span>
		</p>
		{#if doc.problem}
			<div
				class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md bg-warning/10 px-3 py-1.5 text-xs/5 text-warning-text-strong ring-1 ring-warning/30"
				data-testid="doc-problem"
			>
				<AlertTriangleIcon class="size-3.5 shrink-0" aria-hidden="true" />
				{#if doc.problem.kind === 'invalid_metadata'}
					<span>Details not recognized: {doc.problem.field} ({doc.problem.message})</span>
					<a
						href={resolve(`${docHref(projectId, doc.id)}?edit=details` as '/')}
						class="font-semibold text-link hover:underline"
					>
						Fix
					</a>
				{:else}
					<span>
						Duplicate display key {doc.problem.key} also used by {doc.problem.others
							.map((o) => `${o.title} (${o.scopeLabel})`)
							.join(', ')}.
					</span>
					<a
						href={resolve(`${docHref(projectId, doc.id)}?edit=details` as '/')}
						class="font-semibold text-link hover:underline"
					>
						Resolve key…
					</a>
				{/if}
			</div>
		{/if}
	</div>
	{#if showMove && canEdit && isTreeDocument(doc.kind, doc.scope) && sectionCount > 1 && onmove}
		<div class="shrink-0">
			<Button
				type="button"
				variant="outline"
				size="sm"
				class={mobileButton}
				onclick={() => onmove(doc)}
			>
				Move…
			</Button>
		</div>
	{/if}
</li>
