<script lang="ts">
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import SectionHead from '$lib/components/SectionHead.svelte';
	import { deriveTriageItems, executeDecisionAction, type TriageItem } from '$lib/modules/inbox';
	import { useShellData } from '$lib/platform/workspace/shell-data-context';
	import { toAppError } from '$lib/proto';

	let {
		organizationId,
		projectId
	}: {
		organizationId: string;
		projectId: string;
	} = $props();

	let shellData: ReturnType<typeof useShellData> | undefined;
	try {
		shellData = useShellData();
	} catch {
		shellData = undefined;
	}

	let selectedId = $state<string | undefined>(undefined);
	let busyId = $state<string | undefined>(undefined);

	const projectItems = $derived.by<TriageItem[]>(() => {
		if (!shellData?.snapshot) return [];
		const all = deriveTriageItems(
			shellData.workItems,
			shellData.snapshot,
			shellData.ownerId,
			shellData.inboxItems
		);
		return all.filter((item) => item.state === 'open' && item.projectId === projectId);
	});

	const selected = $derived(
		(selectedId ? projectItems.find((i) => i.id === selectedId) : undefined) ?? projectItems[0]
	);

	async function runPrimary(item: TriageItem) {
		if (busyId || !item.decision || !shellData?.snapshot) return;
		busyId = item.id;
		try {
			const res = await executeDecisionAction(
				organizationId,
				item.decision,
				'primary',
				shellData.snapshot,
				''
			);
			if (res.message) toast.success(res.message);
			await shellData.refresh();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			busyId = undefined;
		}
	}
</script>

<section class="pj-att-section flex flex-col gap-3" aria-labelledby="pj-att-heading">
	<SectionHead
		id="pj-att-heading"
		title="Needs your attention"
		reportTitle="Decisions Waiting on You"
		count={projectItems.length}
	>
		{#snippet action()}
			<a
				href={resolve(`/inbox?project=${encodeURIComponent(projectId)}` as '/inbox')}
				class="inline-flex items-center gap-0.5 text-xs font-semibold text-[var(--exec-accent-ink)]"
			>
				<span>Open in Inbox</span>
				<ChevronRight class="size-3.5" aria-hidden="true" />
			</a>
		{/snippet}
	</SectionHead>

	{#if projectItems.length === 0}
		<div class="pj-att-clear exec-card flex items-center gap-2.5 px-4 py-3 text-sm">
			<CheckCircle2 class="size-4 shrink-0 text-[var(--exec-ok)]" aria-hidden="true" />
			<span class="text-[var(--exec-muted)]">
				All clear — nothing on this project is waiting on you right now.
			</span>
		</div>
	{:else}
		<div class="pj-att-split exec-card">
			<ul class="pj-att-queue" role="list" aria-label="Project decisions">
				{#each projectItems as item (item.id)}
					{@const active = item.id === selected?.id}
					<li>
						<button
							type="button"
							aria-current={active ? 'true' : undefined}
							onclick={() => (selectedId = item.id)}
							class={['pj-att-row w-full text-left', active && 'is-selected']}
						>
							<span class="pj-att-row-head">
								<span class="pj-att-av" aria-hidden="true">{item.actorInitials}</span>
								<strong class="truncate text-xs text-[var(--exec-ink)]">{item.actorName}</strong>
								<span class="text-xs text-[var(--exec-muted)]">{item.typeLabel}</span>
								<time class="ml-auto text-xs text-[var(--exec-dim)] tabular-nums">
									{item.ageLabel}
								</time>
							</span>
							<span class="pj-att-row-title">{item.title}</span>
							{#if item.expiresLabel}
								<span class="pj-att-row-exp">
									<Clock class="size-3" aria-hidden="true" />
									<span>{item.expiresLabel}</span>
								</span>
							{/if}
						</button>
					</li>
				{/each}
			</ul>

			{#if selected}
				<div class="pj-att-detail">
					<div class="flex flex-wrap items-center gap-2 text-xs text-[var(--exec-muted)]">
						<span class="pj-att-kind">{selected.kindDisplay}</span>
						{#if selected.workRef}
							<span class="font-mono text-[var(--exec-dim)]">· {selected.workRef}</span>
						{/if}
						{#if selected.expiresLabel}
							<span class="text-[var(--exec-warn)]">· {selected.expiresLabel}</span>
						{/if}
					</div>
					<h4 class="text-base font-semibold text-[var(--exec-ink)]">{selected.title}</h4>
					{#if selected.why}
						<p class="text-sm leading-relaxed text-[var(--exec-body)]">{selected.why}</p>
					{/if}
					{#if selected.recommendation}
						<div class="pj-att-rec">
							<strong class="text-xs text-[var(--exec-ink)]">Recommendation</strong>
							<p class="text-xs leading-relaxed text-[var(--exec-body)]">
								{selected.recommendation}
							</p>
						</div>
					{/if}
					<div class="flex flex-wrap items-center gap-2 pt-1">
						{#if selected.decision}
							<button
								type="button"
								disabled={busyId === selected.id}
								onclick={() => void runPrimary(selected)}
								class="exec-cta inline-flex min-h-9 items-center gap-1.5"
							>
								<span class="exec-cta-fill" aria-hidden="true"></span>
								<span class="exec-cta-inner">
									<Check class="size-3.5" aria-hidden="true" />
									<span>{selected.decision.primaryAction.label}</span>
								</span>
							</button>
						{/if}
						<a
							href={resolve(`/inbox?item=${encodeURIComponent(selected.id)}` as '/inbox')}
							class="pj-att-open-btn"
						>
							<span>Review in Inbox</span>
							<ExternalLink class="size-3.5" aria-hidden="true" />
						</a>
					</div>
				</div>
			{/if}
		</div>
	{/if}
</section>

<style>
	.pj-att-split {
		display: grid;
		grid-template-columns: minmax(240px, 38%) minmax(0, 1fr);
		overflow: hidden;
	}
	@media (max-width: 820px) {
		.pj-att-split {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	.pj-att-queue {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 8px;
		max-height: 280px;
		overflow-y: auto;
		background: var(--exec-well);
		border-inline-end: 1px solid var(--exec-border);
	}
	@media (max-width: 820px) {
		.pj-att-queue {
			border-inline-end: 0;
			border-bottom: 1px solid var(--exec-border);
		}
	}
	.pj-att-row {
		display: grid;
		gap: 4px;
		padding: 10px;
		border-radius: var(--exec-sm-radius, 6px);
		cursor: pointer;
		transition: background-color 120ms ease;
	}
	.pj-att-row:hover {
		background: var(--triage-row-hover, var(--exec-surface-hi));
	}
	.pj-att-row.is-selected {
		background: var(--triage-row-selected, var(--exec-accent-soft));
	}
	.pj-att-row-head {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
	}
	.pj-att-av {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 22px;
		height: 22px;
		border-radius: 999px;
		background: var(--exec-avatar-bg);
		color: var(--exec-ink);
		font-size: 0.65rem;
		font-weight: 700;
		flex: none;
	}
	.pj-att-row-title {
		padding-inline-start: 28px;
		color: var(--exec-ink);
		font-size: 0.875rem;
		font-weight: 600;
		line-height: 1.35;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.pj-att-row-exp {
		padding-inline-start: 28px;
		display: inline-flex;
		align-items: center;
		gap: 4px;
		font-size: 0.72rem;
		color: var(--tone-warn-ink);
	}
	.pj-att-detail {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 16px 20px;
	}
	.pj-att-kind {
		font-weight: 600;
		color: var(--exec-accent-ink);
	}
	.pj-att-rec {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 10px 12px;
		border-radius: var(--exec-sm-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.pj-att-open-btn {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		min-height: 36px;
		padding: 0 12px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		font-size: 0.8125rem;
		font-weight: 600;
	}
	.pj-att-open-btn:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
	}
	@media (prefers-reduced-motion: reduce) {
		.pj-att-open-btn {
			transition: none;
		}
	}
</style>
