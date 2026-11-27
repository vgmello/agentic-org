<!--
  @component
  Reader layout for DocumentPage: Section rail, Markdown body, Code links, and Led to / Mentioned in footer.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import FileCodeIcon from '@lucide/svelte/icons/file-code';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { gutters } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { expectedSectionsFor, headingSlug } from '../../present';
	import type { ProjectDocsOverview, ProjectDocumentDetail } from '../../types';
	import DocMarkdown from '../../components/DocMarkdown.svelte';

	let {
		uid,
		doc,
		kinds
	}: {
		uid: string;
		doc: ProjectDocumentDetail;
		kinds: ProjectDocsOverview['kinds'];
	} = $props();

	const sectionChecklist = $derived(expectedSectionsFor(doc.kind, doc.bodyMarkdown, kinds));
</script>

<div class="grid grid-cols-1 gap-8 py-6 min-[1024px]:grid-cols-[14rem_minmax(0,1fr)] {gutters}">
	<nav aria-label="Document sections" class="flex flex-col gap-1" data-testid="doc-section-rail">
		<p class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase">Sections</p>
		<ul role="list" class="mt-1 flex flex-col gap-1">
			{#each sectionChecklist as sec (sec.heading)}
				<li class="text-xs/5">
					{#if sec.present}
						<a
							href={`#doc-sec-${headingSlug(sec.heading)}`}
							class="block rounded-sm py-1 text-fg-secondary hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
						>
							{sec.heading}
						</a>
						{#if sec.empty && sec.hint}
							<p class="text-fg-muted">{sec.hint}</p>
						{/if}
					{:else}
						<p class="py-1 text-fg-muted">{sec.heading} — {sec.hint}</p>
					{/if}
				</li>
			{/each}
		</ul>
	</nav>

	<div class="flex min-w-0 flex-col gap-8">
		<article aria-label="Document body" data-testid="doc-body">
			<DocMarkdown body={doc.bodyMarkdown} baseLevel={4} />
		</article>

		{#if doc.codeLinks.length > 0}
			<section
				aria-labelledby="{uid}-codelinks-h"
				class="border-t border-line-subtle pt-5"
				data-testid="doc-code-links"
			>
				<h4 id="{uid}-codelinks-h" class="text-sm/6 font-semibold text-fg">
					Code links ({doc.codeLinks.length})
				</h4>
				<ul role="list" class="mt-2 flex flex-col gap-2">
					{#each doc.codeLinks as cl (cl.id)}
						<li
							class="flex flex-wrap items-start justify-between gap-2 rounded-lg bg-surface-raised p-3 text-xs/5 ring-1 ring-line"
						>
							<div class="flex min-w-0 items-start gap-2">
								<FileCodeIcon class="mt-0.5 size-4 shrink-0 text-fg-muted" aria-hidden="true" />
								<div class="min-w-0">
									<p class="font-mono text-fg [overflow-wrap:anywhere]">
										{cl.repositoryKey} · {cl.path}{cl.symbol ? `#${cl.symbol}` : ''}
									</p>
									{#if cl.note}
										<p class="mt-0.5 text-fg-muted">{cl.note}</p>
									{/if}
								</div>
							</div>
							{#if cl.stale}
								<StatusPill label="Path moved or missing" class={tone.owner} />
							{/if}
						</li>
					{/each}
				</ul>
			</section>
		{/if}

		<footer
			class="grid grid-cols-1 gap-6 border-t border-line-subtle pt-6 min-[640px]:grid-cols-2"
			data-testid="doc-why-footer"
		>
			{#each [{ id: 'ledto', title: `Led to (${doc.ledTo.length})`, empty: 'No work items or documents link this document as their Why yet.', items: doc.ledTo }, { id: 'mentioned', title: `Mentioned in (${doc.mentionedIn.length})`, empty: 'Not mentioned elsewhere yet.', items: doc.mentionedIn }] as col (col.id)}
				<section aria-labelledby="{uid}-{col.id}-h">
					<h4 id="{uid}-{col.id}-h" class="text-sm/6 font-semibold text-fg">{col.title}</h4>
					{#if col.items.length === 0}
						<p class="mt-1 text-xs/5 text-fg-muted">{col.empty}</p>
					{:else}
						<ul role="list" class="mt-2 flex flex-col gap-1.5 text-sm/6">
							{#each col.items as item (item.id)}
								<li class="flex items-baseline justify-between gap-2">
									<a
										href={resolve(item.href as '/')}
										class="text-link hover:underline [overflow-wrap:anywhere]"
									>
										{#if item.ref || item.key}
											<span class="mr-1 font-mono text-xs/5">{item.ref ?? item.key}</span>
										{/if}
										{item.title}
									</a>
									<span class="shrink-0 text-xs/5 text-fg-muted">{item.status}</span>
								</li>
							{/each}
						</ul>
					{/if}
				</section>
			{/each}
		</footer>
	</div>
</div>
