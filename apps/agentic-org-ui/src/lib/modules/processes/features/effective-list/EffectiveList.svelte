<!--
  @component
  The inheritance view (processes-ux §2, PROC-9): one row per key, matched across levels, with where the document
  comes from (a level word, an icon and its team or project), its revision, how it relates to the level above
  (replaces, added here, inherited), whether it is not overridable, and whether it applies. A row's problem (a refused
  replacement, details not recognized, a key clash) is written under it in the spec's words, with Fix or Rename… when
  the viewer may change it. The row's title opens the document.
  Wide rows show the source and state in columns; below 640 px they stack under the title.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import Glyph from '../../components/Glyph.svelte';
	import SourceBadge from '../../components/SourceBadge.svelte';
	import { appliesText, clashParts, documentHref, problemText, relationText } from '../../present';
	import type { DocumentKind, EffectiveRow, Place } from '../../types';

	let {
		rows,
		place,
		kind,
		canEdit = false,
		paused = false,
		pausedReasonId,
		labelledby,
		onopen,
		onrename
	}: {
		rows: EffectiveRow[];
		place: Place;
		kind: DocumentKind;
		/** The viewer may change documents: Fix and Rename… are offered. */
		canEdit?: boolean;
		/** Changes wait (the repository can't be reached): the actions say so through `pausedReasonId`. */
		paused?: boolean;
		pausedReasonId?: string;
		labelledby?: string;
		onopen: (row: EffectiveRow, opener: HTMLElement) => void;
		onrename?: (row: EffectiveRow, opener: HTMLElement) => void;
	} = $props();

	const rowKey = (row: EffectiveRow) => row.document.key ?? row.document.id;
</script>

<div class="@container">
	<div
		class="hidden border-b border-line px-4 py-3 text-xs/5 font-semibold text-fg-muted @min-[44rem]:flex @min-[44rem]:gap-x-4 min-[640px]:px-6 min-[1024px]:px-8"
		aria-hidden="true"
	>
		<span class="min-w-0 flex-1">{kind === 'standard' ? 'Standard' : 'Process'}</span>
		<span class="w-[clamp(12rem,36%,22rem)] shrink-0">Comes from</span>
		<span class="w-28 shrink-0">State</span>
	</div>
	<ul role="list" class="divide-y divide-line-subtle" aria-labelledby={labelledby}>
		{#each rows as row (row.id)}
			{@const doc = row.document}
			{@const problem = problemText(row)}
			<li
				class="relative flex flex-wrap items-start gap-x-4 gap-y-2 px-4 py-4 transition-colors duration-150 hover:bg-tint/[0.02] motion-reduce:transition-none @min-[44rem]:flex-nowrap min-[640px]:px-6 min-[1024px]:px-8"
				data-testid="process-row"
				data-key={rowKey(row)}
				data-row={row.id}
				data-document={doc.id}
				data-applies={row.applies}
			>
				<div class="min-w-0 flex-1 basis-full @min-[44rem]:basis-auto">
					<p class="text-sm/6 font-semibold text-fg">
						<button
							type="button"
							class="rounded-sm text-left [overflow-wrap:anywhere] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
							aria-describedby="{row.id}-meta"
							onclick={(e) => onopen(row, e.currentTarget)}
							><span class="absolute inset-0" aria-hidden="true"></span>{doc.title}</button
						>
					</p>
					{#if doc.key}<p class="mt-1 font-mono text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
							{doc.key}
						</p>{/if}
					{#if problem}
						<div class="relative mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
							<p
								class="flex min-w-0 items-start gap-x-1.5 text-xs/5 text-danger-text"
								data-testid="problem"
							>
								<Glyph name="warning" class="mt-0.5 text-danger" />
								<span class="min-w-0 [overflow-wrap:anywhere]">
									{#if row.problem?.kind === 'clash'}
										{@const parts = clashParts(doc.kind, row.problem.scopeName)}
										{parts.before} <code class="font-mono text-fg-bright">{doc.key}</code>
										{parts.after}
									{:else}
										{problem}
									{/if}
								</span>
							</p>
							{#if canEdit && row.problem?.kind === 'invalid'}
								<Button
									variant="outline"
									size="sm"
									class="min-w-16 {mobileButton}"
									aria-label="Fix {doc.title}"
									disabled={paused}
									aria-describedby={paused ? pausedReasonId : undefined}
									href={paused ? undefined : resolve(documentHref(doc.id, place, kind) as '/')}
									>Fix</Button
								>
							{:else if canEdit && row.problem?.kind === 'clash' && onrename}
								<Button
									variant="outline"
									size="sm"
									class={mobileButton}
									aria-label="Rename {doc.title}…"
									disabled={paused}
									aria-describedby={paused ? pausedReasonId : undefined}
									onclick={(e: MouseEvent) => onrename(row, e.currentTarget as HTMLElement)}
									>Rename…</Button
								>
							{/if}
						</div>
					{/if}
				</div>
				<div
					id="{row.id}-meta"
					class="flex min-w-0 basis-full flex-col items-start gap-y-1 @min-[44rem]:w-[clamp(12rem,36%,22rem)] @min-[44rem]:shrink-0 @min-[44rem]:basis-auto"
				>
					<SourceBadge document={doc} />
					<p class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs/5 text-fg-muted">
						{#if !row.problem && place.kind !== 'organization'}
							<span class="inline-flex items-center gap-x-1" data-testid="relation">
								<Glyph name={row.replaces ? 'replaces' : row.here ? 'added' : doc.level} />
								{relationText(row)}
							</span>
						{/if}
						{#if !doc.overridable}
							<span class="inline-flex items-center gap-x-1 text-fg-secondary" data-testid="locked">
								<Glyph name="lock" class="text-fg-secondary" />not overridable
							</span>
						{/if}
					</p>
				</div>
				<p
					class="inline-flex shrink-0 items-center gap-x-1 text-xs/5 @min-[44rem]:w-28 @min-[44rem]:text-sm/6 {row.applies
						? 'text-positive-text'
						: 'text-fg-muted'}"
					data-testid="applies"
				>
					<Glyph
						name={row.applies ? 'applies' : 'notApplied'}
						class={row.applies ? 'text-positive' : 'text-quiet'}
					/>{appliesText(row)}
				</p>
			</li>
		{/each}
	</ul>
</div>
