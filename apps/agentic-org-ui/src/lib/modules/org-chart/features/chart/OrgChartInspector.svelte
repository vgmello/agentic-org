<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import { resolve } from '$app/paths';
	import { isBuiltInHandle } from '$lib/proto';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import X from '@lucide/svelte/icons/x';
	import Avatar from '$lib/components/Avatar.svelte';
	import type { CanvasNodeData } from '../../canvas-layout';
	import type { ChartEmployee } from '../../types';
	import type { NodeAction } from './ChartNode.svelte';

	let {
		selectedData,
		parentItem,
		directChildren,
		teamMembers,
		canMove = false,
		onClose,
		onJumpTo,
		onAction
	}: {
		selectedData: CanvasNodeData;
		parentItem: CanvasNodeData | undefined;
		directChildren: readonly CanvasNodeData[];
		teamMembers: readonly ChartEmployee[];
		canMove?: boolean;
		onClose: () => void;
		onJumpTo: (id: string) => void;
		onAction?: (action: NodeAction, employee: ChartEmployee) => void;
	} = $props();
</script>

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
		<Tooltip.Provider delayDuration={150}>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							onclick={onClose}
							aria-label="Close inspector"
							class="org-zoom-btn"
						>
							<X class="size-4" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						role="tooltip"
						side="left"
						sideOffset={6}
						class="z-[100] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
					>
						Close inspector
					</Tooltip.Content>
				</Tooltip.Portal>
			</Tooltip.Root>
		</Tooltip.Provider>
	</div>

	<div class="org-insp-body">
		<div class="org-insp-row">
			<span class="org-insp-k">{selectedData.kind === 'team' ? 'Composition' : 'Home team'}</span>
			<span class="org-insp-v">{selectedData.meta}</span>
		</div>

		{#if parentItem}
			<div class="org-insp-row">
				<span class="org-insp-k">
					{selectedData.kind === 'team' ? 'Parent team' : 'Reports to'}
				</span>
				<button type="button" onclick={() => onJumpTo(parentItem.id)} class="org-insp-chip">
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
						<button type="button" onclick={() => onJumpTo(child.id)} class="org-insp-chip">
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
		{#if selectedData.employee && onAction}
			<button
				type="button"
				onclick={() => onAction('manager', selectedData.employee!)}
				class="org-bar-btn w-full justify-center"
			>
				Change manager…
			</button>
			{#if canMove && selectedData.employee.homeTeam && !isBuiltInHandle(selectedData.employee.kind, selectedData.employee.handle)}
				<button
					type="button"
					onclick={() => onAction('move', selectedData.employee!)}
					class="org-bar-btn w-full justify-center"
				>
					Move home team…
				</button>
			{/if}
		{/if}
	</div>
</aside>

<style>
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
