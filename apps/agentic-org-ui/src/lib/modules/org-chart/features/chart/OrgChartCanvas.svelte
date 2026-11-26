<script lang="ts">
	import { tick } from 'svelte';
	import { resolve } from '$app/paths';
	import { SvelteSet } from 'svelte/reactivity';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import Home from '@lucide/svelte/icons/home';
	import Maximize2 from '@lucide/svelte/icons/maximize-2';
	import Minus from '@lucide/svelte/icons/minus';
	import Plus from '@lucide/svelte/icons/plus';
	import Search from '@lucide/svelte/icons/search';
	import Users from '@lucide/svelte/icons/users';
	import X from '@lucide/svelte/icons/x';
	import Avatar from '$lib/components/Avatar.svelte';
	import type { OrgSnapshot } from '$lib/proto';
	import {
		buildPeopleCanvasItems,
		buildTeamCanvasItems,
		hotPathIds,
		layoutCanvasTree,
		type CanvasNodeData,
		type OrgCanvasMode
	} from '../../canvas-layout';
	import type { ChartEmployee } from '../../types';
	import type { NodeAction } from './ChartNode.svelte';

	let {
		employees,
		ownerId,
		snapshot,
		canMove = false,
		onaction
	}: {
		employees: readonly ChartEmployee[];
		ownerId: string;
		snapshot: OrgSnapshot | null;
		canMove?: boolean;
		onaction?: (action: NodeAction, employee: ChartEmployee) => void;
	} = $props();

	let mode = $state<OrgCanvasMode>('teams');
	let searchQuery = $state('');
	let searchOpen = $state(false);
	let selectedId = $state<string | undefined>(undefined);
	let inspectorOpen = $state(true);

	const collapsed = new SvelteSet<string>();

	let viewportEl = $state<HTMLElement | null>(null);
	let panX = $state(24);
	let panY = $state(24);
	let zoom = $state(1);
	let dragging = $state(false);
	let dragStart = { x: 0, y: 0, panX: 0, panY: 0 };

	const items = $derived<CanvasNodeData[]>(
		mode === 'teams'
			? buildTeamCanvasItems(snapshot, employees)
			: buildPeopleCanvasItems(employees, ownerId)
	);
	const layout = $derived(layoutCanvasTree(items, collapsed));
	const effectiveSelectedId = $derived(
		selectedId && layout.byId.has(selectedId) ? selectedId : layout.nodes[0]?.data.id
	);
	const selectedData = $derived(
		effectiveSelectedId ? layout.byId.get(effectiveSelectedId) : undefined
	);
	const hotIds = $derived(hotPathIds(layout.byId, effectiveSelectedId));

	const searchResults = $derived.by(() => {
		const q = searchQuery.trim().toLowerCase();
		if (!q) return [];
		return items
			.filter(
				(it) =>
					it.title.toLowerCase().includes(q) ||
					it.subtitle.toLowerCase().includes(q) ||
					it.meta.toLowerCase().includes(q)
			)
			.slice(0, 8);
	});

	const employeeById = $derived.by(() => {
		const out: Record<string, ChartEmployee> = {};
		for (const e of employees) out[e.id] = e;
		return out;
	});

	const directChildren = $derived(
		effectiveSelectedId ? items.filter((it) => it.parentId === effectiveSelectedId) : []
	);
	const parentItem = $derived(
		selectedData?.parentId ? layout.byId.get(selectedData.parentId) : undefined
	);
	const teamMembers = $derived(
		selectedData?.kind === 'team' && selectedData.memberIds
			? selectedData.memberIds
					.map((id) => employeeById[id])
					.filter((m): m is ChartEmployee => Boolean(m))
			: []
	);

	function clampZoom(next: number): number {
		return Math.min(1.8, Math.max(0.45, Math.round(next * 100) / 100));
	}

	function fitView() {
		if (!viewportEl) {
			zoom = 1;
			panX = 24;
			panY = 24;
			return;
		}
		const rect = viewportEl.getBoundingClientRect();
		if (rect.width <= 0 || rect.height <= 0) return;
		const scaleX = (rect.width - 48) / Math.max(layout.width, 320);
		const scaleY = (rect.height - 72) / Math.max(layout.height, 240);
		const nextZoom = clampZoom(Math.min(1, scaleX, scaleY));
		zoom = nextZoom;
		panX = Math.max(16, Math.round((rect.width - layout.width * nextZoom) / 2));
		panY = 24;
	}

	function centerOnNode(id: string) {
		const node = layout.nodes.find((n) => n.data.id === id);
		if (!node || !viewportEl) return;
		const rect = viewportEl.getBoundingClientRect();
		panX = Math.round(rect.width / 2 - (node.x + node.w / 2) * zoom);
		panY = Math.round(rect.height / 2 - (node.y + node.h / 2) * zoom);
	}

	function expandAncestors(id: string) {
		let cur = layout.byId.get(id)?.parentId;
		let guard = 0;
		while (cur && guard++ < 40) {
			collapsed.delete(cur);
			cur = layout.byId.get(cur)?.parentId;
		}
	}

	function jumpTo(id: string) {
		expandAncestors(id);
		selectedId = id;
		inspectorOpen = true;
		searchOpen = false;
		searchQuery = '';
		void tick().then(() => centerOnNode(id));
	}

	function toggleCollapse(id: string, event: MouseEvent) {
		event.stopPropagation();
		if (collapsed.has(id)) collapsed.delete(id);
		else collapsed.add(id);
	}

	function switchMode(next: OrgCanvasMode) {
		mode = next;
		selectedId = undefined;
		searchQuery = '';
		void tick().then(() => fitView());
	}

	function handlePointerDown(event: PointerEvent) {
		if ((event.target as HTMLElement | null)?.closest('button, a, input')) return;
		dragging = true;
		dragStart = { x: event.clientX, y: event.clientY, panX, panY };
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
	}

	function handlePointerMove(event: PointerEvent) {
		if (!dragging) return;
		panX = dragStart.panX + (event.clientX - dragStart.x);
		panY = dragStart.panY + (event.clientY - dragStart.y);
	}

	function handlePointerUp() {
		dragging = false;
	}

	function handleWheel(event: WheelEvent) {
		if (!event.ctrlKey && !event.metaKey) return;
		event.preventDefault();
		const delta = event.deltaY < 0 ? 0.1 : -0.1;
		zoom = clampZoom(zoom + delta);
	}

	function edgePath(e: { x1: number; y1: number; x2: number; y2: number }): string {
		const midY = e.y1 + (e.y2 - e.y1) * 0.5;
		return `M ${e.x1} ${e.y1} C ${e.x1} ${midY}, ${e.x2} ${midY}, ${e.x2} ${e.y2}`;
	}

	$effect(() => {
		void layout.nodes.length;
		void tick().then(() => {
			if (viewportEl && panX === 24 && panY === 24 && zoom === 1) fitView();
		});
	});
</script>

<div class="org-canvas-shell" data-testid="org-chart-canvas">
	<div class="org-canvas-bar">
		<div class="org-mode-seg" role="group" aria-label="Org chart view">
			<button
				type="button"
				aria-pressed={mode === 'teams'}
				onclick={() => switchMode('teams')}
				class={['org-seg-btn', mode === 'teams' && 'is-on']}
			>
				Teams
			</button>
			<button
				type="button"
				aria-pressed={mode === 'people'}
				onclick={() => switchMode('people')}
				class={['org-seg-btn', mode === 'people' && 'is-on']}
			>
				People
			</button>
		</div>

		<div class="org-search-wrap">
			<Search class="org-search-ico size-3.5" aria-hidden="true" />
			<input
				type="search"
				value={searchQuery}
				oninput={(e) => {
					searchQuery = e.currentTarget.value;
					searchOpen = true;
				}}
				onfocus={() => (searchOpen = true)}
				onkeydown={(e) => {
					if (e.key === 'Escape') {
						searchOpen = false;
						searchQuery = '';
					} else if (e.key === 'Enter' && searchResults[0]) {
						jumpTo(searchResults[0].id);
					}
				}}
				placeholder={mode === 'teams' ? 'Jump to team…' : 'Jump to person or agent…'}
				aria-label={mode === 'teams' ? 'Jump to team' : 'Jump to person or agent'}
				class="org-search-input"
			/>
			{#if searchOpen && searchResults.length > 0}
				<ul class="org-search-pop" role="listbox" aria-label="Matching nodes">
					{#each searchResults as hit (hit.id)}
						<li>
							<button type="button" onclick={() => jumpTo(hit.id)} class="org-search-hit">
								<Avatar name={hit.avatarName} size="sm" />
								<span class="min-w-0 flex-1 truncate">
									<strong class="block truncate text-xs text-[var(--exec-ink)]">{hit.title}</strong>
									<span class="block truncate text-[11px] text-[var(--exec-muted)]"
										>{hit.subtitle}</span
									>
								</span>
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</div>

		<div class="org-bar-actions">
			{#if collapsed.size > 0}
				<button type="button" onclick={() => collapsed.clear()} class="org-bar-btn">
					Expand all
				</button>
			{/if}
			<button type="button" onclick={fitView} class="org-bar-btn">
				<Maximize2 class="size-3.5" aria-hidden="true" />
				<span>Fit view</span>
			</button>
			<button
				type="button"
				onclick={() => (inspectorOpen = !inspectorOpen)}
				aria-pressed={inspectorOpen}
				class="org-bar-btn"
			>
				{inspectorOpen ? 'Hide inspector' : 'Inspector'}
			</button>
		</div>
	</div>

	<div class="org-stage-grid">
		<div
			bind:this={viewportEl}
			role="region"
			aria-label="Interactive organization chart canvas"
			class={['org-viewport', dragging && 'is-dragging']}
			onpointerdown={handlePointerDown}
			onpointermove={handlePointerMove}
			onpointerup={handlePointerUp}
			onpointercancel={handlePointerUp}
			onwheel={handleWheel}
		>
			{#if layout.nodes.length === 0}
				<div class="org-empty">
					<Users class="size-6 opacity-50" aria-hidden="true" />
					<p>No {mode === 'teams' ? 'teams' : 'members'} to display yet.</p>
				</div>
			{:else}
				<div
					class="org-world"
					style="width: {layout.width}px; height: {layout.height}px; transform: translate({panX}px, {panY}px) scale({zoom});"
				>
					<svg class="org-edges" width={layout.width} height={layout.height} aria-hidden="true">
						{#each layout.edges as edge (edge.id)}
							{@const isHot = hotIds.has(edge.fromId) && hotIds.has(edge.toId)}
							<path d={edgePath(edge)} class={['org-edge', isHot && 'is-hot']} fill="none" />
						{/each}
					</svg>

					{#each layout.nodes as node (node.data.id)}
						{@const isSelected = node.data.id === effectiveSelectedId}
						{@const isHot = hotIds.has(node.data.id)}
						<div
							class={['org-node-wrap', isSelected && 'is-selected', isHot && 'is-hot']}
							style="left: {node.x}px; top: {node.y}px; width: {node.w}px; height: {node.h}px;"
						>
							<button
								type="button"
								onclick={() => {
									selectedId = node.data.id;
									inspectorOpen = true;
								}}
								aria-pressed={isSelected}
								class="org-node-card"
							>
								<div class="org-node-head">
									<Avatar name={node.data.avatarName} size="sm" />
									<span class="min-w-0 flex-1">
										<span class="org-node-title block truncate">{node.data.title}</span>
										<span class="org-node-sub block truncate">{node.data.subtitle}</span>
									</span>
									{#if node.data.badge}
										<span class={['org-node-badge', `tone-${node.data.badgeTone ?? 'muted'}`]}>
											{node.data.badge}
										</span>
									{/if}
								</div>
								<div class="org-node-meta truncate">{node.data.meta}</div>
							</button>

							{#if node.data.childCount > 0}
								<button
									type="button"
									onclick={(e) => toggleCollapse(node.data.id, e)}
									aria-label={node.collapsed
										? `Expand ${node.hiddenCount} under ${node.data.title}`
										: `Collapse under ${node.data.title}`}
									class="org-collapse-pill"
								>
									{node.collapsed ? `+${node.hiddenCount}` : '−'}
								</button>
							{/if}
						</div>
					{/each}
				</div>
			{/if}

			<div class="org-zoom-dock" role="group" aria-label="Canvas zoom controls">
				<button
					type="button"
					onclick={() => {
						zoom = 1;
						fitView();
					}}
					aria-label="Reset view"
					class="org-zoom-btn"
				>
					<Home class="size-3.5" aria-hidden="true" />
				</button>
				<button
					type="button"
					onclick={() => (zoom = clampZoom(zoom - 0.15))}
					aria-label="Zoom out"
					class="org-zoom-btn"
				>
					<Minus class="size-3.5" aria-hidden="true" />
				</button>
				<span class="org-zoom-pct tabular-nums">{Math.round(zoom * 100)}%</span>
				<button
					type="button"
					onclick={() => (zoom = clampZoom(zoom + 0.15))}
					aria-label="Zoom in"
					class="org-zoom-btn"
				>
					<Plus class="size-3.5" aria-hidden="true" />
				</button>
				<button type="button" onclick={fitView} aria-label="Fit chart" class="org-zoom-btn">
					<Maximize2 class="size-3.5" aria-hidden="true" />
				</button>
			</div>
		</div>

		{#if inspectorOpen && selectedData}
			<aside class="org-inspector" aria-label="Selected node details">
				<div class="org-insp-head">
					<div class="flex min-w-0 items-center gap-2.5">
						<Avatar name={selectedData.avatarName} size="md" />
						<div class="min-w-0">
							<h3 class="truncate text-sm font-semibold text-[var(--exec-ink)]">
								{selectedData.title}
							</h3>
							<p class="truncate text-xs text-[var(--exec-muted)]">{selectedData.subtitle}</p>
						</div>
					</div>
					<button
						type="button"
						onclick={() => (inspectorOpen = false)}
						aria-label="Close inspector"
						class="org-zoom-btn"
					>
						<X class="size-4" aria-hidden="true" />
					</button>
				</div>

				<div class="org-insp-body">
					<div class="org-insp-row">
						<span class="org-insp-k"
							>{selectedData.kind === 'team' ? 'Composition' : 'Home team'}</span
						>
						<span class="org-insp-v">{selectedData.meta}</span>
					</div>

					{#if parentItem}
						<div class="org-insp-row">
							<span class="org-insp-k">
								{selectedData.kind === 'team' ? 'Parent team' : 'Reports to'}
							</span>
							<button type="button" onclick={() => jumpTo(parentItem.id)} class="org-insp-chip">
								<span>{parentItem.title}</span>
								<ChevronRight class="size-3" aria-hidden="true" />
							</button>
						</div>
					{/if}

					{#if directChildren.length > 0}
						<div class="org-insp-section">
							<span class="org-insp-k">
								{selectedData.kind === 'team' ? 'Sub-teams' : 'Direct reports'} ({directChildren.length})
							</span>
							<div class="org-insp-chips">
								{#each directChildren as child (child.id)}
									<button type="button" onclick={() => jumpTo(child.id)} class="org-insp-chip">
										<span>{child.title}</span>
									</button>
								{/each}
							</div>
						</div>
					{/if}

					{#if selectedData.kind === 'team' && teamMembers.length > 0}
						<div class="org-insp-section">
							<span class="org-insp-k">Members ({teamMembers.length})</span>
							<ul class="org-insp-members" role="list">
								{#each teamMembers.slice(0, 8) as m (m.id)}
									<li>
										<a
											href={resolve(`/members/${encodeURIComponent(m.id)}` as '/')}
											class="org-insp-member-link"
										>
											<Avatar name={m.name} size="sm" />
											<span class="min-w-0 flex-1 truncate">{m.name}</span>
											<span class="text-[11px] text-[var(--exec-muted)]">
												{m.kind === 'agent' ? 'Agent' : 'Person'}
											</span>
										</a>
									</li>
								{/each}
							</ul>
						</div>
					{/if}
				</div>

				<div class="org-insp-actions">
					<a href={resolve(selectedData.href as '/teams')} class="org-insp-primary">
						<span>Open {selectedData.kind === 'team' ? 'team' : 'profile'}</span>
						<ExternalLink class="size-3.5" aria-hidden="true" />
					</a>
					{#if selectedData.employee && onaction}
						<button
							type="button"
							onclick={() => onaction('manager', selectedData.employee!)}
							class="org-bar-btn w-full justify-center"
						>
							Change manager…
						</button>
						{#if canMove && selectedData.employee.homeTeam}
							<button
								type="button"
								onclick={() => onaction('move', selectedData.employee!)}
								class="org-bar-btn w-full justify-center"
							>
								Move home team…
							</button>
						{/if}
					{/if}
				</div>
			</aside>
		{/if}
	</div>
</div>

<style>
	.org-canvas-shell {
		display: flex;
		flex-direction: column;
		border: 1px solid var(--exec-border);
		border-radius: var(--exec-panel-radius, 12px);
		background: var(--exec-well);
		overflow: hidden;
	}
	.org-canvas-bar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
		padding: 10px 14px;
		border-bottom: 1px solid var(--exec-border);
		background: var(--exec-surface);
	}
	.org-mode-seg {
		display: inline-flex;
		padding: 2px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.org-seg-btn {
		padding: 4px 12px;
		border-radius: calc(var(--exec-ctrl-radius, 8px) - 2px);
		font-size: 0.75rem;
		font-weight: 600;
		color: var(--exec-muted);
		cursor: pointer;
	}
	.org-seg-btn.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.org-search-wrap {
		position: relative;
		display: flex;
		align-items: center;
		gap: 6px;
		flex: 1 1 200px;
		max-width: 280px;
		padding: 0 10px;
		min-height: 32px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.org-search-wrap :global(.org-search-ico) {
		color: var(--exec-muted);
		flex: none;
	}
	.org-search-input {
		flex: 1 1 auto;
		min-width: 0;
		border: 0;
		background: transparent;
		color: var(--exec-ink);
		font-size: 0.8125rem;
		outline: none;
	}
	.org-search-pop {
		position: absolute;
		top: calc(100% + 4px);
		left: 0;
		right: 0;
		z-index: 30;
		padding: 4px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-surface);
		box-shadow:
			inset 0 0 0 1px var(--exec-border-hi),
			var(--tooltip-shadow);
	}
	.org-search-hit {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		padding: 6px 8px;
		border-radius: 6px;
		text-align: left;
		cursor: pointer;
	}
	.org-search-hit:hover {
		background: var(--exec-accent-soft);
	}
	.org-bar-actions {
		display: flex;
		align-items: center;
		gap: 6px;
		margin-inline-start: auto;
	}
	.org-bar-btn {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		min-height: 32px;
		padding: 0 10px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
	}
	.org-bar-btn:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
	}
	.org-stage-grid {
		display: grid;
		grid-template-columns: minmax(0, 1fr) auto;
		min-height: 520px;
		max-height: 70dvh;
	}
	@media (max-width: 900px) {
		.org-stage-grid {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	.org-viewport {
		position: relative;
		overflow: hidden;
		min-height: 500px;
		cursor: grab;
		user-select: none;
		background-image: radial-gradient(var(--exec-border) 1px, transparent 1px);
		background-size: 20px 20px;
	}
	.org-viewport.is-dragging {
		cursor: grabbing;
	}
	.org-world {
		position: relative;
		transform-origin: 0 0;
	}
	.org-edges {
		position: absolute;
		inset: 0;
		pointer-events: none;
	}
	.org-edge {
		stroke: var(--exec-border-hi);
		stroke-width: 1.5;
	}
	.org-edge.is-hot {
		stroke: var(--tone-accent-hi);
		stroke-width: 2.25;
	}
	.org-node-wrap {
		position: absolute;
	}
	.org-node-card {
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		width: 100%;
		height: 100%;
		padding: 10px 12px;
		border-radius: var(--exec-inset-radius, 10px);
		background: var(--exec-surface);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		text-align: left;
		cursor: pointer;
		transition:
			box-shadow 120ms ease,
			background-color 120ms ease;
	}
	.org-node-card:hover {
		background: var(--exec-surface-hi);
	}
	.org-node-wrap.is-hot .org-node-card {
		box-shadow: inset 0 0 0 1.5px var(--exec-accent-ring);
	}
	.org-node-wrap.is-selected .org-node-card {
		background: var(--exec-surface-hi);
		box-shadow:
			inset 0 0 0 2px var(--tone-accent-hi),
			0 8px 20px -10px var(--side-panel-scrim);
	}
	.org-node-head {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.org-node-title {
		font-size: 0.8125rem;
		font-weight: 600;
		color: var(--exec-ink);
	}
	.org-node-sub {
		font-size: 0.72rem;
		color: var(--exec-muted);
	}
	.org-node-meta {
		font-size: 0.72rem;
		color: var(--exec-muted);
		padding-top: 4px;
		border-top: 1px solid var(--exec-border);
	}
	.org-node-badge {
		flex: none;
		padding: 1px 6px;
		border-radius: 999px;
		font-size: 0.65rem;
		font-weight: 600;
	}
	.org-node-badge.tone-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
	}
	.org-node-badge.tone-ok {
		background: var(--tone-ok-bg);
		color: var(--tone-ok-ink);
	}
	.org-node-badge.tone-muted {
		background: var(--exec-well);
		color: var(--exec-muted);
	}
	.org-collapse-pill {
		position: absolute;
		left: 50%;
		bottom: -11px;
		transform: translateX(-50%);
		min-width: 24px;
		height: 20px;
		padding: 0 6px;
		border-radius: 999px;
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.6875rem;
		font-weight: 600;
		cursor: pointer;
	}
	.org-collapse-pill:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
	}
	.org-zoom-dock {
		position: absolute;
		left: 50%;
		bottom: 14px;
		transform: translateX(-50%);
		display: inline-flex;
		align-items: center;
		gap: 2px;
		padding: 4px;
		border-radius: 999px;
		background: var(--exec-surface);
		box-shadow:
			inset 0 0 0 1px var(--exec-border-hi),
			var(--tooltip-shadow);
	}
	.org-zoom-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		border-radius: 999px;
		color: var(--exec-body);
		cursor: pointer;
	}
	.org-zoom-btn:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.org-zoom-pct {
		min-width: 44px;
		text-align: center;
		font-size: 0.72rem;
		font-weight: 600;
		color: var(--exec-muted);
	}
	.org-empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 8px;
		height: 100%;
		color: var(--exec-muted);
		font-size: 0.875rem;
	}
	.org-inspector {
		width: 300px;
		border-inline-start: 1px solid var(--exec-border);
		background: var(--exec-surface);
		display: flex;
		flex-direction: column;
		overflow-y: auto;
	}
	@media (max-width: 900px) {
		.org-inspector {
			width: 100%;
			border-inline-start: 0;
			border-top: 1px solid var(--exec-border);
		}
	}
	.org-insp-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 14px 16px;
		border-bottom: 1px solid var(--exec-border);
	}
	.org-insp-body {
		padding: 14px 16px;
		display: flex;
		flex-direction: column;
		gap: 14px;
		flex: 1 1 auto;
	}
	.org-insp-row,
	.org-insp-section {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.org-insp-k {
		font-size: 0.7rem;
		font-weight: 650;
		letter-spacing: 0.06em;
		text-transform: uppercase;
		color: var(--exec-dim);
	}
	.org-insp-v {
		font-size: 0.8125rem;
		color: var(--exec-body);
	}
	.org-insp-chips {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	.org-insp-chip {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		padding: 3px 9px;
		border-radius: 999px;
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.75rem;
		font-weight: 500;
		cursor: pointer;
	}
	.org-insp-chip:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
	}
	.org-insp-members {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.org-insp-member-link {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 5px 6px;
		border-radius: 6px;
		font-size: 0.8125rem;
		color: var(--exec-body);
	}
	.org-insp-member-link:hover {
		background: var(--exec-well);
		color: var(--exec-ink);
	}
	.org-insp-actions {
		padding: 12px 16px 16px;
		border-top: 1px solid var(--exec-border);
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.org-insp-primary {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		min-height: 34px;
		padding: 0 12px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-cta-bg);
		color: var(--exec-on-accent);
		font-size: 0.8125rem;
		font-weight: 600;
	}
</style>
