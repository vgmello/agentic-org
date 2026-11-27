<!--
  @component
  One process or standard (PROC-8, PROC-9): its source, scope, lifecycle, owner, key and revision; why it reads this
  way (every level's document for its key, highest first, and what happened to each); its text; and, for a viewer
  who may change it, Edit, Override for this team or project (not for a document marked not overridable), and the
  difference from the document it replaces, one click away. Back closes the sheet only (app-shell-ux §6).
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Sheet,
		SheetClose,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { registerModalBack } from '$lib/platform/shell/modal-back';
	import DiffView from '../../components/DiffView.svelte';
	import DocumentText from '../../components/DocumentText.svelte';
	import Glyph from '../../components/Glyph.svelte';
	import SourceBadge from '../../components/SourceBadge.svelte';
	import When from '../../components/When.svelte';
	import { wrapButton } from '../../components/classes';
	import {
		KIND_WORD,
		LEVEL_WORD,
		LIFECYCLE_WORD,
		canOverride,
		documentHref,
		newDocumentHref,
		problemText,
		sourceText
	} from '../../present';
	import type { EffectiveRow, Layer, Place, ProcessDocument } from '../../types';

	let {
		open = $bindable(false),
		row,
		document: shown,
		place,
		placeName,
		canEdit = false,
		paused = false,
		onCloseAutoFocus
	}: {
		open: boolean;
		/** A row of the inheritance view, with its chain; or `document` alone (a draft or deprecated one). */
		row?: EffectiveRow;
		document?: ProcessDocument;
		place: Place;
		/** "Web", for "Override for Web". */
		placeName: string;
		canEdit?: boolean;
		paused?: boolean;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	const doc = $derived(row?.document ?? shown);
	const chain = $derived<Layer[]>(row?.chain ?? []);
	let comparing = $state(false);
	$effect(() => {
		if (open) return registerModalBack(() => (open = false));
	});

	const OUTCOME: Record<Layer['outcome'], string> = {
		applies: 'Applies',
		replaced: 'Replaced',
		refused: 'Refused',
		invalid: 'Left out',
		clash: 'Left out'
	};
	function layerNote(layer: Layer, index: number): string {
		const next = chain[index + 1]?.document;
		if (layer.outcome === 'replaced' && next) return `Replaced by ${sourceText(next)}.`;
		if (layer.outcome === 'refused') {
			const by = chain.find((l) => l.outcome === 'applies')?.document;
			return by
				? `${sourceText(by)} marked ${by.title} not overridable, so this version isn't applied.`
				: "Not applied: the document above can't be replaced.";
		}
		if (layer.outcome === 'invalid')
			return `Details not recognized (${layer.document.invalid?.field ?? 'front matter'}): left out until it's fixed.`;
		if (layer.outcome === 'clash')
			return 'Another document uses this key here: left out until one is renamed.';
		return index === 0 ? 'Written here.' : 'Wins: it is the lowest level with this key.';
	}
	const scopeText = (d: ProcessDocument) =>
		d.level === 'organization' ? 'Organization' : `${LEVEL_WORD[d.level]} ${d.scopeName ?? ''}`;
	const updatedBy = (d: ProcessDocument) =>
		d.updated.byName ?? (d.updated.byKind === 'system' ? 'Agentic Org' : 'Unknown');
</script>

<Sheet bind:open onOpenChangeComplete={(next) => !next && (comparing = false)}>
	<SheetContent
		side="right"
		showCloseButton={false}
		{onCloseAutoFocus}
		class="w-full overflow-y-auto data-[side=right]:w-full data-[side=right]:sm:max-w-[30rem]"
	>
		{#if doc}
			<SheetHeader class="pr-14">
				<SheetTitle class="text-base/7 font-semibold text-fg [overflow-wrap:anywhere]"
					>{doc.title}</SheetTitle
				>
				<SheetDescription class="flex flex-wrap items-center gap-2">
					<span class="sr-only"
						>{doc.kind === 'standard' ? 'Standard' : 'Organization process'},</span
					>
					<SourceBadge document={doc} />
					{#if doc.status !== 'current'}<span class="text-xs/5 text-fg-secondary"
							>{LIFECYCLE_WORD[doc.status]}</span
						>{/if}
					{#if !doc.overridable}<span
							class="inline-flex items-center gap-x-1 text-xs/5 text-fg-secondary"
							><Glyph name="lock" class="text-fg-secondary" />not overridable</span
						>{/if}
				</SheetDescription>
			</SheetHeader>

			<div class="space-y-6 px-4 pb-8 text-sm/6">
				{#if row && problemText(row)}
					<p class="flex items-start gap-x-1.5 text-danger-text" data-testid="sheet-problem">
						<Glyph name="warning" class="mt-1 text-danger" />
						<span class="min-w-0 [overflow-wrap:anywhere]">{problemText(row)}</span>
					</p>
				{/if}

				{#if canEdit}
					<div class="flex flex-wrap gap-3">
						{#if row && canOverride(row, place)}
							<Button
								class="{mobileButton} {wrapButton}"
								disabled={paused}
								href={paused
									? undefined
									: resolve(newDocumentHref(place, doc.kind, { from: doc.id }) as '/')}
								>Override for {placeName}</Button
							>
						{/if}
						<Button
							variant="outline"
							class="{mobileButton} {wrapButton}"
							disabled={paused}
							href={paused ? undefined : resolve(documentHref(doc.id, place, doc.kind) as '/')}
							>{doc.invalid ? 'Fix' : 'Edit'}<span class="sr-only"> {doc.title}</span></Button
						>
						{#if row?.replaces}
							<Button
								variant="outline"
								class="{mobileButton} {wrapButton}"
								aria-expanded={comparing}
								onclick={() => (comparing = !comparing)}
								>{comparing
									? 'Hide the difference'
									: `Compare with ${sourceText(row.replaces)}`}</Button
							>
						{/if}
					</div>
					{#if paused}<p class="text-xs/5 text-fg-muted">
							Changes wait until the organization repository can be reached.
						</p>{/if}
				{:else if row?.replaces}
					<Button
						variant="outline"
						class="{mobileButton} {wrapButton}"
						aria-expanded={comparing}
						onclick={() => (comparing = !comparing)}
						>{comparing
							? 'Hide the difference'
							: `Compare with ${sourceText(row.replaces)}`}</Button
					>
				{/if}

				{#if comparing && row?.replaces}
					<DiffView
						before={row.replaces.bodyMarkdown}
						after={doc.bodyMarkdown}
						label="From {sourceText(row.replaces)} rev {row.replaces.revision} to {sourceText(
							doc
						)} rev {doc.revision}"
					/>
				{/if}

				<dl
					class="grid grid-cols-1 gap-x-4 gap-y-1 text-fg-secondary min-[400px]:grid-cols-[auto_minmax(0,1fr)]"
				>
					<dt class="text-fg-muted">Kind</dt>
					<dd>{doc.kind === 'standard' ? 'Standard' : 'Organization process'}</dd>
					<dt class="text-fg-muted">Key</dt>
					<dd class="font-mono [overflow-wrap:anywhere]">{doc.key ?? 'None'}</dd>
					<dt class="text-fg-muted">Scope</dt>
					<dd class="[overflow-wrap:anywhere]">{scopeText(doc)}</dd>
					<dt class="text-fg-muted">Lifecycle</dt>
					<dd>{LIFECYCLE_WORD[doc.status]}</dd>
					<dt class="text-fg-muted">Owner</dt>
					<dd class="[overflow-wrap:anywhere]">{doc.owner?.name ?? 'No owner'}</dd>
					<dt class="text-fg-muted">Revision</dt>
					<dd class="tabular-nums">{doc.revision}</dd>
					<dt class="text-fg-muted">Can be replaced below</dt>
					<dd>{doc.overridable ? 'Yes' : 'No: not overridable'}</dd>
					<dt class="text-fg-muted">Changed</dt>
					<dd><When at={doc.updated.at} /> by {updatedBy(doc)}</dd>
					<dt class="text-fg-muted">File</dt>
					<dd class="font-mono text-xs/6 [overflow-wrap:anywhere]">{doc.path}</dd>
				</dl>

				{#if chain.length > 0}
					<section aria-labelledby="why-heading" data-testid="why">
						<h3 id="why-heading" class="font-semibold text-fg">Why it reads this way</h3>
						<ol role="list" class="mt-2 space-y-2">
							{#each chain as layer, i (layer.document.id)}
								<li
									class="flex gap-x-2 rounded-md bg-surface-raised p-2 outline-1 -outline-offset-1 outline-line"
									data-testid="why-layer"
									data-outcome={layer.outcome}
								>
									<Glyph
										name={layer.outcome === 'applies'
											? 'applies'
											: layer.outcome === 'replaced'
												? 'notApplied'
												: 'warning'}
										class="mt-1 {layer.outcome === 'applies'
											? 'text-positive'
											: layer.outcome === 'replaced'
												? 'text-quiet'
												: 'text-danger'}"
									/>
									<div class="min-w-0">
										<p class="flex flex-wrap items-center gap-2 text-fg">
											<SourceBadge document={layer.document} />
											<span class="font-medium">{OUTCOME[layer.outcome]}</span>
										</p>
										<p class="text-fg-secondary [overflow-wrap:anywhere]">{layerNote(layer, i)}</p>
									</div>
								</li>
							{/each}
						</ol>
					</section>
				{/if}

				<section aria-labelledby="text-heading">
					<h3 id="text-heading" class="font-semibold text-fg">
						The {KIND_WORD[doc.kind]}{doc.invalid ? ' (still readable)' : ''}
					</h3>
					<div class="mt-2"><DocumentText body={doc.bodyMarkdown} /></div>
				</section>
			</div>
		{/if}
		<span class="absolute top-4 right-4 inline-flex">
			<SheetClose>
				{#snippet child({ props })}
					<Button
						variant="ghost"
						size="icon-sm"
						class="peer focus-visible:ring-0 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-solid focus-visible:outline-focus max-[639px]:size-11"
						{...props}
					>
						<MiniIcon name="xMark" class="size-5" />
						<span class="sr-only">Close</span>
					</Button>
				{/snippet}
			</SheetClose>
			<span
				role="tooltip"
				data-tooltip-content=""
				class="pointer-events-none invisible absolute top-1/2 right-full z-[60] mr-1.5 -translate-y-1/2 whitespace-nowrap rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg opacity-0 shadow-md ring-1 ring-line transition-opacity delay-150 peer-hover:visible peer-hover:opacity-100 peer-focus-visible:visible peer-focus-visible:opacity-100 motion-reduce:transition-none"
				>Close</span
			>
		</span>
	</SheetContent>
</Sheet>
