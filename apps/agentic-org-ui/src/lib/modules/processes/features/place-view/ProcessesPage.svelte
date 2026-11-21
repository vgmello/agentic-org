<!--
  @component
  Organization settings › Processes (processes-ux §2, decision 7): "Processes · <place>", the organization's processes
  and standards as two tabs, each opening with its definition, for the organization or any team, project or agent
  chosen with Show. The place and the tab are in the URL: choosing a place is a history entry, Back and Forward
  move between the places viewed, and reload keeps them (PROC-31). A project's development process is named above
  the list and never listed in it (PROC-35, PROC-37).
  States: loading (skeleton rows), unavailable, error with Retry, a place that isn't here, nothing written yet (with
  the review-and-merge template), the repository unreachable (the last copy, changes paused), and a viewer who may
  not change processes (no actions, app-shell-ux §0.1).
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { mobileButton, selectClass } from '$lib/components/classes';
	import { focusHeading } from '$lib/components/focus';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { accountZone } from '$lib/modules/preferences';
	import { pageTitle } from '$lib/nav';
	import Glyph from '../../components/Glyph.svelte';
	import StaleNotice from '../../components/StaleNotice.svelte';
	import PageTitle from '../../components/PageTitle.svelte';
	import { wrapButton } from '../../components/classes';
	import {
		CHANGES_PAUSED,
		DEV_PROCESSES_URL,
		EMPTY_TEXT,
		EMPTY_TITLE,
		KIND_DEFINITION,
		KIND_TAB,
		KIND_WORD,
		LIFECYCLE_WORD,
		NO_CURRENT_WORK,
		REVIEW_AND_MERGE,
		agentWorkText,
		devProcessHref,
		listHref,
		newDocumentHref,
		parseKind,
		parsePlace,
		placeLabel,
		placeParam,
		samePlace,
		sourceText
	} from '../../present';
	import type { DocumentKind, EffectiveRow, PlaceView, ProcessDocument } from '../../types';
	import EffectiveList from '../effective-list/EffectiveList.svelte';
	import DocumentSheet from '../inspect/DocumentSheet.svelte';
	import RenameDialog from '../rename/RenameDialog.svelte';
	import { readPlace } from './load';
	import { takeArrival } from './arrival';

	let { organizationId }: { organizationId: string } = $props();

	const place = $derived(parsePlace(page.url.searchParams.get('place')));
	const kind = $derived(parseKind(page.url.searchParams.get('kind')));
	const placeKey = $derived(placeParam(place) ?? 'organization');

	const resource = new Resource(() => readPlace(organizationId, place));
	// Another place (Show, Back, Forward) is another read; the tab is the same read.
	let loadedKey = untrack(() => placeKey);
	$effect(() => {
		if (placeKey === loadedKey) return;
		loadedKey = placeKey;
		void resource.reload();
	});
	$effect(() => void accountZone.ensure(organizationId));

	/** The read of the place in the URL; an answer for the place before stays hidden while the next one loads. */
	const read = $derived(
		resource.data && samePlace(resource.data.place, place) ? resource.data : undefined
	);
	const view = $derived<PlaceView | undefined>(read && 'view' in read ? read.view : undefined);
	const notFound = $derived(!!read && 'notFound' in read);
	/** The Show options stay while another place loads, so the select keeps focus and its choices. */
	const options = $derived(
		resource.data && 'view' in resource.data ? resource.data.view.options : undefined
	);
	const option = $derived(view?.options.find((o) => samePlace(o.place, place)));
	const label = $derived(
		option ? placeLabel(option) : place.kind === 'organization' ? 'Organization' : ''
	);
	const title = $derived(label ? `Processes · ${label}` : 'Processes');
	const rows = $derived<EffectiveRow[]>(
		view ? (kind === 'standard' ? view.standards : view.processes) : []
	);
	const inactive = $derived(view?.inactive.filter((d) => d.kind === kind) ?? []);
	const canEdit = $derived(!!view?.viewer.canEdit && !!view?.writable);
	const paused = $derived(!!view?.stale);
	const empty = $derived(!!view && rows.length === 0 && inactive.length === 0);

	// Reached from a save: focus the saved document's row once it is listed, and say where it went (P2-4).
	const arrival = takeArrival();
	let arrived = $state<string | undefined>();
	$effect(() => {
		if (!arrival || !view || arrived !== undefined) return;
		arrived = arrival.message;
		// After the shell's own focus move to its h1 (a tick after navigation).
		setTimeout(() => {
			const target = document.querySelector<HTMLElement>(
				`[data-document="${CSS.escape(arrival.documentId)}"] button`
			);
			if (target) target.focus();
			else focusHeading('processes-heading');
		}, 0);
	});

	// Back and Forward between places keep the page; focus never falls to the body (app-shell-ux §0).
	afterNavigate((move) => {
		if (move.type !== 'popstate' || move.from?.url.pathname !== move.to?.url.pathname) return;
		setTimeout(() => {
			const active = document.activeElement;
			if (!active || active === document.body) focusHeading('processes-heading');
		}, 0);
	});

	function choose(value: string) {
		const next = parsePlace(value);
		// A history entry, so Back returns to the place before (PROC-31); focus stays on Show.
		void goto(resolve(listHref(next, kind) as '/'), { keepFocus: true, noScroll: true });
	}

	function chooseTab(event: MouseEvent, next: DocumentKind) {
		if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
			return;
		event.preventDefault();
		void goto(resolve(listHref(place, next) as '/'), { keepFocus: true, noScroll: true });
	}

	let sheetOpen = $state(false);
	let sheetRow = $state.raw<EffectiveRow | undefined>();
	let sheetDocument = $state.raw<ProcessDocument | undefined>();
	let opener: HTMLElement | null = null;
	let renamer = $state<ReturnType<typeof RenameDialog>>();

	function openRow(row: EffectiveRow, from: HTMLElement) {
		opener = from;
		sheetRow = row;
		sheetDocument = undefined;
		sheetOpen = true;
	}
	function openDocument(doc: ProcessDocument, from: HTMLElement) {
		opener = from;
		sheetRow = undefined;
		sheetDocument = doc;
		sheetOpen = true;
	}
	function restoreFocus(event: Event) {
		event.preventDefault();
		if (opener?.isConnected) opener.focus();
		else focusHeading('processes-heading');
	}
	function rename(row: EffectiveRow, from: HTMLElement) {
		opener = from;
		renamer?.start(row.document);
	}
	async function renamed(doc: ProcessDocument) {
		await resource.reload();
		// The row's Rename… is gone: focus its title instead.
		setTimeout(() => {
			const button = document.querySelector<HTMLElement>(
				`[data-row="${CSS.escape(doc.key ?? doc.id)}"] button, [data-row$="${CSS.escape(doc.id)}"] button`
			);
			(button ?? document.getElementById('processes-heading'))?.focus();
		}, 0);
	}
</script>

{#if resource.state === 'unavailable'}
	<!-- This server doesn't serve processes: only the honest placeholder, never a frame that looks usable. -->
	<NotBuilt title="Processes" place="Processes" area="Organization settings" />
{:else}
	<PageTitle title={pageTitle(label || undefined, 'Processes')} />

	<PageHeader id="processes-heading" {title} class="[&_h2:focus]:outline-none">
		{#snippet description()}
			<p>Organization processes and standards: how agents work, written down and inherited.</p>
			<p class="mt-1">
				A project's way of building software is its <a
					href={resolve(DEV_PROCESSES_URL as '/')}
					class="rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					>development process</a
				>, chosen on the project.
			</p>
		{/snippet}
		{#snippet actions()}
			{#if resource.state !== 'error' && resource.state !== 'unsupported'}
				<div class="flex w-full flex-wrap items-end gap-3 min-[640px]:w-auto">
					<div class="w-full min-[640px]:w-60">
						<label for="processes-show" class="block text-sm/6 font-medium text-fg">Show</label>
						<select
							id="processes-show"
							aria-label="Show processes for"
							class="mt-1 {selectClass}"
							value={placeParam(place) ?? 'organization'}
							disabled={!options}
							onchange={(e) => choose(e.currentTarget.value)}
						>
							{#if options}
								{#each options as o (placeParam(o.place) ?? 'organization')}
									<option value={placeParam(o.place) ?? 'organization'}>{placeLabel(o)}</option>
								{/each}
							{:else}
								<option value={placeParam(place) ?? 'organization'}>{label || 'Loading…'}</option>
							{/if}
						</select>
					</div>
					{#if view && canEdit && !empty}
						<div class="flex max-w-60 flex-col items-start gap-y-1">
							<Button
								class={mobileButton}
								disabled={paused}
								aria-describedby={paused ? 'processes-paused' : undefined}
								href={paused ? undefined : resolve(newDocumentHref(place, kind) as '/')}
								><MiniIcon name="plus" class="-ml-0.5 size-5" />New {KIND_WORD[kind]}</Button
							>
							{#if paused}
								<p
									id="processes-paused"
									class="text-xs/5 text-fg-muted"
									data-testid="paused-reason"
								>
									{CHANGES_PAUSED}
								</p>
							{/if}
						</div>
					{/if}
				</div>
			{/if}
		{/snippet}
	</PageHeader>

	{#if arrived}
		<p
			role="status"
			class="px-4 pt-4 text-sm/6 text-fg-secondary min-[640px]:px-6 min-[1024px]:px-8"
			data-testid="arrival"
		>
			{arrived}
		</p>
	{/if}
	{#if view?.stale}
		<StaleNotice
			stale={view.stale}
			retrying={resource.reloading}
			onretry={() => resource.reload()}
		/>
	{/if}

	{#if resource.state !== 'error' && resource.state !== 'unsupported'}
		<nav
			aria-label="Processes and standards"
			class="flex gap-x-6 overflow-x-auto border-b border-line px-4 min-[640px]:px-6 min-[1024px]:px-8"
		>
			{#each ['process', 'standard'] as const as tab (tab)}
				{@const current = tab === kind}
				{@const count = view
					? (tab === 'standard' ? view.standards : view.processes).length
					: undefined}
				<a
					href={resolve(listHref(place, tab) as '/')}
					aria-current={current ? 'page' : undefined}
					onclick={(e) => chooseTab(e, tab)}
					class="-mb-px flex min-h-11 items-center gap-x-2 border-b-2 py-3 text-sm/6 font-semibold whitespace-nowrap focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus {current
						? 'border-brand text-fg'
						: 'border-transparent text-fg-muted hover:border-line-strong hover:text-fg'}"
				>
					{KIND_TAB[tab]}
					{#if count}<span
							class="rounded-full bg-surface-overlay px-2 text-xs/5 font-medium text-fg-secondary"
							>{count}</span
						>{/if}
				</a>
			{/each}
		</nav>
		<p
			class="px-4 pt-4 text-sm/6 text-fg-secondary min-[640px]:px-6 min-[1024px]:px-8"
			data-testid="definition"
		>
			{KIND_TAB[kind]}: {KIND_DEFINITION[kind]}
		</p>
	{/if}

	{#if resource.state === 'error'}
		<ErrorState
			title="Couldn't load the processes"
			message={resource.message}
			onretry={() => resource.reload()}
			retrying={resource.reloading}
		/>
	{:else if resource.state === 'unsupported'}
		<div class="px-4 py-12 min-[640px]:px-6 min-[1024px]:px-8" data-testid="resource-unsupported">
			<h3 class="text-sm/6 font-semibold text-fg">Not available in this version</h3>
			<p class="mt-1 max-w-xl text-sm/6 text-fg-muted [overflow-wrap:anywhere]">
				{resource.message || "This version of the office doesn't support processes."}
			</p>
		</div>
	{:else if notFound}
		<EmptyState
			title="That place isn't here"
			description="The team, project or agent in this address isn't in this organization. It may have been archived or removed."
		>
			{#snippet action()}
				<Button
					variant="outline"
					class="{mobileButton} {wrapButton}"
					href={resolve(listHref({ kind: 'organization' }, kind) as '/')}
					>Show the organization's {KIND_WORD[kind] === 'process'
						? 'processes'
						: 'standards'}</Button
				>
			{/snippet}
		</EmptyState>
	{:else if !view}
		<SkeletonRows label={kind === 'standard' ? 'standards' : 'processes'} rows={4} />
	{:else}
		{#if view.developmentProcess || view.agent}
			<div class="space-y-1 px-4 pt-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
				{#if view.agent}
					<p data-testid="agent-work">
						{view.agent.work
							? agentWorkText(option?.name ?? '', view.agent.work.ref, view.agent.work.title)
							: NO_CURRENT_WORK}
					</p>
				{/if}
				{#if view.developmentProcess}
					<p data-testid="dev-process-line">
						{view.agent?.work?.projectName
							? `Its project, ${view.agent.work.projectName}, follows`
							: 'This project follows'}
						<a
							href={resolve(devProcessHref(view.developmentProcess.id) as '/')}
							class="rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
							>{view.developmentProcess.name}</a
						>. Its skills are shown there.
					</p>
				{/if}
			</div>
		{/if}

		{#if empty}
			<EmptyState level={3} title={EMPTY_TITLE[kind]} description={EMPTY_TEXT[kind]}>
				{#snippet action()}
					{#if canEdit}
						{#if kind === 'process'}
							<Button
								class="{mobileButton} {wrapButton}"
								disabled={paused}
								href={paused
									? undefined
									: resolve(
											newDocumentHref(place, 'process', { template: REVIEW_AND_MERGE.key }) as '/'
										)}>Use the review-and-merge template</Button
							>
							<Button
								variant="outline"
								class={mobileButton}
								disabled={paused}
								href={paused ? undefined : resolve(newDocumentHref(place, 'process') as '/')}
								>New process</Button
							>
						{:else}
							<Button
								class={mobileButton}
								disabled={paused}
								href={paused ? undefined : resolve(newDocumentHref(place, 'standard') as '/')}
								><MiniIcon name="plus" class="-ml-0.5 size-5" />New standard</Button
							>
						{/if}
					{/if}
				{/snippet}
			</EmptyState>
		{:else}
			<h3 id="processes-list-heading" class="sr-only">
				{KIND_TAB[kind]} that apply to {label}
			</h3>
			{#if rows.length > 0}
				<div class="mt-4">
					<EffectiveList
						{rows}
						{place}
						{kind}
						{canEdit}
						{paused}
						pausedReasonId="processes-paused"
						labelledby="processes-list-heading"
						onopen={openRow}
						onrename={rename}
					/>
				</div>
			{:else}
				<p class="px-4 py-6 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
					None of the organization's {KIND_WORD[kind] === 'process' ? 'processes' : 'standards'} apply
					here yet.
				</p>
			{/if}

			{#if inactive.length > 0}
				<section aria-labelledby="processes-inactive" class="mt-8" data-testid="inactive">
					<h3
						id="processes-inactive"
						class="border-b border-line px-4 pb-3 text-sm/6 font-semibold text-fg min-[640px]:px-6 min-[1024px]:px-8"
					>
						Not applied: drafts and deprecated
					</h3>
					<ul role="list" class="divide-y divide-line-subtle">
						{#each inactive as doc (doc.id)}
							<li
								class="relative flex flex-wrap items-start gap-x-4 gap-y-1 px-4 py-4 hover:bg-tint/[0.02] min-[640px]:px-6 min-[1024px]:px-8"
								data-testid="inactive-row"
								data-document={doc.id}
							>
								<p class="min-w-0 flex-1 basis-48 text-sm/6 font-semibold text-fg">
									<button
										type="button"
										class="rounded-sm text-left [overflow-wrap:anywhere] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
										onclick={(e) => openDocument(doc, e.currentTarget)}
										><span class="absolute inset-0" aria-hidden="true"></span>{doc.title}</button
									>
								</p>
								<p class="inline-flex items-center gap-x-1 text-xs/5 text-fg-muted">
									<Glyph name="notApplied" class="text-quiet" />
									{LIFECYCLE_WORD[doc.status]} · {sourceText(doc)} rev {doc.revision}
								</p>
							</li>
						{/each}
					</ul>
				</section>
			{/if}
		{/if}
	{/if}
{/if}

<DocumentSheet
	bind:open={sheetOpen}
	row={sheetRow}
	document={sheetDocument}
	{place}
	placeName={option?.name ?? ''}
	{canEdit}
	{paused}
	onCloseAutoFocus={restoreFocus}
/>
<RenameDialog
	bind:this={renamer}
	{organizationId}
	onrenamed={renamed}
	onCloseAutoFocus={restoreFocus}
/>
