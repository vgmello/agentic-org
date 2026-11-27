<!--
  @component
  A team's Work tab (Prototype Release 1149b4f1, offices-and-teams-ux §7.2):
  its tasks as interactive divided work rows backed by the frame-owned work resource, with shared top-header
  scoped search (`/` focuses, `Escape` keeps query intact and focuses heading) and inline reading-panel support
  for direct clicks and `?item=<ref>` deep links.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import ScopedSearchInput from '$lib/components/ScopedSearchInput.svelte';
	import WorkReadingPanel from '$lib/components/WorkReadingPanel.svelte';
	import {
		WORK_PRIORITY_LABELS,
		WORK_STATE_LABELS,
		type WorkRow,
		type WorkState
	} from '$lib/modules/members';
	import { WorkRows } from '$lib/modules/members/ui';
	import type { WorkStatusRingTone } from '$lib/modules/work/compact';
	import { WorkDetailPage } from '$lib/modules/workitems/ui';
	import { normalizeRef, taskHref } from '$lib/modules/workitems/present';
	import { isEditableTarget } from '$lib/platform/workspace/shortcuts';
	import { useScopedSearchContext } from '$lib/platform/workspace/scoped-search.svelte';
	import { teamPageContext } from './context';

	const team = teamPageContext();
	const work = team.work;
	const scopedSearch = useScopedSearchContext();

	let query = $state('');
	let openRowRef = $state<string | null>(null);
	let openSnapshot = $state<WorkRow | null>(null);
	let lastTriggerEl: HTMLElement | null = null;
	let dismissedUrlItem = $state<string | null>(null);

	$effect(() => {
		if (!scopedSearch) return;
		const teamName = team.teamName;
		return scopedSearch.register({
			getValue: () => query,
			setValue: (next) => {
				query = next;
			},
			label: `Search work in ${teamName}`,
			placeholder: 'Search work…',
			headingSelector: '#team-heading, #shell-page-title, main h1, main h2'
		});
	});

	const urlItem = $derived(page.url.searchParams.get('item'));
	const requestedRef = $derived(
		openRowRef
			? normalizeRef(openRowRef)
			: urlItem && normalizeRef(urlItem) !== dismissedUrlItem
				? normalizeRef(urlItem)
				: null
	);
	const matchedRow = $derived.by<WorkRow | null>(() => {
		if (!requestedRef) return null;
		const rows = work.state === 'ready' ? (work.data ?? []) : [];
		return rows.find((r) => normalizeRef(r.ref) === requestedRef) ?? null;
	});
	// Keep the reading panel mounted if an already-open item leaves the team list (e.g. cancelled)
	// so the user sees the confirmation inside WorkDetailPage and closing still strips ?item= and restores focus.
	$effect(() => {
		if (!requestedRef) {
			openSnapshot = null;
			return;
		}
		if (matchedRow) {
			openSnapshot = matchedRow;
		}
	});
	const activeOpenRow = $derived<WorkRow | null>(
		matchedRow ??
			(requestedRef && openSnapshot && normalizeRef(openSnapshot.ref) === requestedRef
				? openSnapshot
				: null)
	);

	function readingStatusTone(state: WorkState): WorkStatusRingTone {
		if (state === 'done') return 'done';
		if (state === 'blocked') return 'blocked';
		if (state === 'in_progress') return 'active';
		return 'neutral';
	}

	function readingKindLabel(kind: WorkRow['kind']): 'Task' | 'Bug' | 'Request' {
		if (kind === 'bug') return 'Bug';
		if (kind === 'request') return 'Request';
		return 'Task';
	}

	function readingPriority(priority: WorkRow['priority']): {
		level: 1 | 2 | 3;
		label: string;
		strong: boolean;
	} {
		if (priority === 'urgent') return { level: 3, label: 'Urgent priority', strong: true };
		if (priority === 'high') return { level: 3, label: 'High priority', strong: false };
		if (priority === 'low') return { level: 1, label: 'Low priority', strong: false };
		return { level: 2, label: 'Medium priority', strong: false };
	}

	function openInlineDetail(row: WorkRow, triggerEl: HTMLElement | null) {
		lastTriggerEl =
			triggerEl ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
		openRowRef = row.ref;
		dismissedUrlItem = null;
	}

	function closeInlineDetail(restoreFocus = true) {
		const closedRef = activeOpenRow?.ref ?? openRowRef ?? openSnapshot?.ref ?? null;
		const trigger = lastTriggerEl;
		openRowRef = null;
		openSnapshot = null;
		lastTriggerEl = null;
		if (urlItem) {
			const dismissed = normalizeRef(urlItem);
			dismissedUrlItem = dismissed;
			const nextUrl = new URL(page.url);
			nextUrl.searchParams.delete('item');
			// eslint-disable-next-line svelte/no-navigation-without-resolve -- preserving current already-resolved URL
			void goto(nextUrl, { replaceState: true, noScroll: true, keepFocus: true }).finally(() => {
				if (dismissedUrlItem === dismissed && !page.url.searchParams.get('item')) {
					dismissedUrlItem = null;
				}
			});
		}
		if (!restoreFocus) return;
		void tick().then(() => {
			if (activeOpenRow) return;
			if (trigger?.isConnected && trigger.offsetParent !== null) {
				trigger.focus({ preventScroll: true });
				trigger.scrollIntoView({ block: 'nearest', inline: 'nearest' });
				return;
			}
			if (closedRef) {
				const link = document.querySelector<HTMLElement>(`[data-ref="${CSS.escape(closedRef)}"] a`);
				if (link?.isConnected && link.offsetParent !== null) {
					link.focus({ preventScroll: true });
					link.scrollIntoView({ block: 'nearest', inline: 'nearest' });
					return;
				}
			}
			document.getElementById('team-heading')?.focus({ preventScroll: true });
		});
	}

	function attachDetailEscape() {
		const modalOverlaySelector =
			'[role="dialog"]:not(.work-reading), [role="menu"], [role="alertdialog"], [role="listbox"]';
		const onKeydown = (event: KeyboardEvent) => {
			if (
				event.defaultPrevented ||
				event.isComposing ||
				event.keyCode === 229 ||
				event.key !== 'Escape' ||
				event.repeat ||
				!activeOpenRow
			) {
				return;
			}
			if (document.querySelector(modalOverlaySelector)) return;
			if (isEditableTarget(event.target)) return;
			event.preventDefault();
			closeInlineDetail(true);
		};
		window.addEventListener('keydown', onKeydown);
		return () => {
			window.removeEventListener('keydown', onKeydown);
		};
	}

	function filterWorkRows(rows: WorkRow[], rawQuery: string): WorkRow[] {
		const needle = rawQuery.trim().toLowerCase();
		if (!needle) return rows;
		return rows.filter((r) =>
			[
				r.ref,
				r.title,
				r.project,
				r.assignee,
				r.state,
				WORK_STATE_LABELS[r.state],
				r.priority ? WORK_PRIORITY_LABELS[r.priority] : undefined
			]
				.filter((part): part is string => Boolean(part))
				.some((part) => part.toLowerCase().includes(needle))
		);
	}
</script>

<div {@attach attachDetailEscape}>
	<Loaded
		errorLevel={4}
		resource={work}
		label="the team's work"
		error="Couldn't load the team's work"
	>
		{#snippet children(rows)}
			{#if rows.length === 0}
				<EmptyState
					level={4}
					title="No work yet"
					description="Tasks {team.teamName} is accountable for, or that are assigned to it, show here."
				/>
			{:else}
				{@const filtered = filterWorkRows(rows, query)}
				<div
					class="flex flex-wrap items-center justify-between gap-3 border-b border-line-subtle px-4 py-2.5 min-[640px]:px-6 min-[1024px]:px-8"
				>
					{#if !scopedSearch}
						<ScopedSearchInput
							bind:value={query}
							label="Search work in {team.teamName}"
							placeholder="Search work…"
							headingSelector="#team-heading, #shell-page-title, main h1, main h2"
						/>
					{/if}
					<p class="text-xs/5 text-fg-muted tabular-nums" role="status">
						{#if query.trim()}
							{filtered.length} of {rows.length} {rows.length === 1 ? 'item' : 'items'}
						{:else}
							{rows.length} {rows.length === 1 ? 'item' : 'items'}
						{/if}
					</p>
				</div>
				{#if filtered.length === 0}
					<p class="px-4 py-8 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
						No work matches this search.
					</p>
				{:else}
					<WorkRows rows={filtered} linked={true} onopen={openInlineDetail} />
				{/if}
			{/if}
		{/snippet}
	</Loaded>
</div>

{#if activeOpenRow}
	{@const pri = readingPriority(activeOpenRow.priority)}
	<div class="work-workspace has-work-detail">
		<WorkReadingPanel
			itemId={activeOpenRow.ref}
			refLabel={activeOpenRow.ref}
			kindLabel={readingKindLabel(activeOpenRow.kind)}
			ariaLabel="{activeOpenRow.ref} · {activeOpenRow.title}"
			fullPageHref={taskHref(activeOpenRow.ref)}
			statusTone={matchedRow ? readingStatusTone(activeOpenRow.state) : undefined}
			needsAttention={matchedRow ? activeOpenRow.state === 'blocked' : false}
			priorityLevel={pri.level}
			priorityLabel={pri.label}
			priorityStrong={pri.strong}
			onclose={() => closeInlineDetail(true)}
		>
			{#key activeOpenRow.ref}
				<WorkDetailPage
					organizationId={team.organizationId}
					refParam={activeOpenRow.ref}
					embedded
					onchanged={() => void work.reload()}
				/>
			{/key}
		</WorkReadingPanel>
	</div>
{/if}
