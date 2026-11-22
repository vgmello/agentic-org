<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Ban from '@lucide/svelte/icons/ban';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import Scale from '@lucide/svelte/icons/scale';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Zap from '@lucide/svelte/icons/zap';
	import type { WorkItemSummary } from '$lib/proto';
	import { executeDecisionAction } from '$lib/home/actions';
	import { deriveDecisionSpotlight, type SpotlightDecision } from '$lib/home/model';
	import InboxView from '$lib/modules/inbox/features/view/InboxView.svelte';
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/office/api';
	import { owner } from '$lib/org/model';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import { workApi } from '$lib/work';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const ownerId = $derived(owner(snapshot)?.id ?? 'owner');

	let items = $state.raw<WorkItemSummary[] | undefined>(undefined);
	let filter = $state<'all' | 'request' | 'bug' | 'task'>('all');
	let busyId = $state<string | undefined>(undefined);
	let nowMs = $state(Date.now());

	const allDecisions = $derived(deriveDecisionSpotlight(items ?? [], snapshot, ownerId, nowMs));
	const filtered = $derived(
		filter === 'all' ? allDecisions : allDecisions.filter((d) => d.item.kind === filter)
	);

	async function load() {
		try {
			items = (await workApi.snapshot(organizationId)).items;
			nowMs = Date.now();
		} catch (e) {
			const err = toAppError(e);
			if (err.source === 'api' && (err.code === 'unsupported' || err.code === 'unavailable')) {
				items = [];
			} else {
				toast.error(err.message);
				items = [];
			}
		}
	}

	onMount(() => {
		if (uiPrefs.executiveUi) void load();
	});

	async function act(decision: SpotlightDecision, kind: 'primary' | 'secondary') {
		if (busyId) return;
		busyId = decision.id;
		try {
			const res = await executeDecisionAction(organizationId, decision, kind, snapshot);
			if (res.navigateTo) {
				await goto(resolve(res.navigateTo as '/work'));
				return;
			}
			if (res.message) toast.success(res.message);
			await load();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			busyId = undefined;
		}
	}
</script>

<svelte:head><title>{pageTitle('Inbox')}</title></svelte:head>

{#if uiPrefs.executiveUi}
	<div class="mx-auto flex w-full max-w-[1100px] flex-col gap-5 px-6 pt-3 pb-12 sm:px-8">
		<div class="flex flex-wrap items-center justify-between gap-4">
			<div>
				<h1
					id="shell-page-title"
					tabindex="-1"
					class="text-2xl font-semibold tracking-tight focus:outline-none"
					style="color: var(--exec-ink);"
				>
					Inbox
				</h1>
				<p class="mt-0.5 text-sm" style="color: var(--exec-muted);">
					{allDecisions.length === 0
						? 'All caught up — nothing needs your decision.'
						: `${allDecisions.length} ${allDecisions.length === 1 ? 'item needs' : 'items need'} your decision.`}
				</p>
			</div>
			<div
				class="exec-well inline-flex items-center gap-1 p-1"
				role="tablist"
				aria-label="Filter decisions"
			>
				{#each [['all', 'All'], ['request', 'Requests'], ['bug', 'Bugs'], ['task', 'Blocked']] as [val, label] (val)}
					<button
						type="button"
						role="tab"
						aria-selected={filter === val}
						onclick={() => (filter = val as typeof filter)}
						class={['inbox-tab rounded-lg px-3 py-1.5 text-xs font-semibold', filter === val && 'is-on']}
					>
						{label}
					</button>
				{/each}
			</div>
		</div>

		{#if items === undefined}
			<div class="flex flex-col gap-3" aria-busy="true">
				{#each [1, 2, 3] as n (n)}
					<div class="exec-card h-32 p-5"></div>
				{/each}
			</div>
		{:else if filtered.length === 0}
			<div class="exec-card flex flex-col items-center gap-2.5 px-6 py-16 text-center">
				<span class="inbox-ok-ring flex size-12 items-center justify-center rounded-full">
					<CheckCircle2 class="size-6" aria-hidden="true" />
				</span>
				<h2 class="text-base font-semibold" style="color: var(--exec-ink);">Nothing waiting here</h2>
				<p class="max-w-sm text-sm" style="color: var(--exec-muted);">
					Your agents and teams will surface approvals, verifications, and blockers here when they need you.
				</p>
				<a href={resolve('/home')} class="exec-cta mt-2">
					<span class="exec-cta-fill" aria-hidden="true"></span>
					<span class="exec-cta-inner"><span>Back to Home</span></span>
				</a>
			</div>
		{:else}
			<ul class="flex flex-col gap-3.5">
				{#each filtered as d (d.id)}
					<li class="exec-card flex flex-col gap-3.5 p-5">
						<div class="flex flex-wrap items-center justify-between gap-2">
							<div class="flex flex-wrap items-center gap-2 text-xs" style="color: var(--exec-muted);">
								<span class={['inbox-badge', d.urgent && 'is-urgent']}>{d.badgeText}</span>
								<span class="font-semibold" style="color: var(--exec-ink);">{d.actorName}</span>
								<span aria-hidden="true">·</span>
								<span>{d.projectName}</span>
								<span aria-hidden="true">·</span>
								<span>{d.ageLabel}</span>
							</div>
							<a
								href={resolve(d.detailHref as '/work')}
								class="inline-flex items-center gap-0.5 text-xs font-medium"
								style="color: var(--exec-accent-ink);"
							>
								<span>Open in Work</span>
								<ChevronRight class="size-3.5" aria-hidden="true" />
							</a>
						</div>

						<h2 class="text-base font-semibold" style="color: var(--exec-ink);">{d.title}</h2>
						<p class="text-sm" style="color: var(--exec-body);">{d.recommendation}</p>

						<div class="exec-well grid grid-cols-1 gap-2 p-2.5 sm:grid-cols-3">
							{#each [d.risk, d.impact, d.proof] as cell, i (cell.label + i)}
								<div class="flex items-center gap-2 px-1.5">
									{#if cell.icon === 'shield-check'}
										<ShieldCheck class="size-4 shrink-0" style="color: var(--exec-ok);" aria-hidden="true" />
									{:else if cell.icon === 'shield-alert'}
										<ShieldAlert class="size-4 shrink-0" style="color: var(--exec-warn);" aria-hidden="true" />
									{:else if cell.icon === 'bolt'}
										<Zap class="size-4 shrink-0" style="color: var(--exec-sky);" aria-hidden="true" />
									{:else if cell.icon === 'scale'}
										<Scale class="size-4 shrink-0" style="color: var(--exec-sky);" aria-hidden="true" />
									{:else if cell.icon === 'check-circle'}
										<CheckCircle2 class="size-4 shrink-0" style="color: var(--exec-ok);" aria-hidden="true" />
									{:else}
										<Clock class="size-4 shrink-0" style="color: var(--exec-muted);" aria-hidden="true" />
									{/if}
									<div class="min-w-0">
										<div class="truncate text-xs font-semibold" style="color: var(--exec-ink);">{cell.label}</div>
										<div class="truncate text-[11px]" style="color: var(--exec-muted);">{cell.sub}</div>
									</div>
								</div>
							{/each}
						</div>

						<div class="flex flex-wrap items-center gap-2 pt-1">
							<button
								type="button"
								disabled={busyId === d.id}
								onclick={() => void act(d, 'primary')}
								class="exec-cta"
							>
								<span class="exec-cta-fill" aria-hidden="true"></span>
								<span class="exec-cta-inner">
									<Check class="size-4" aria-hidden="true" />
									<span>{d.primaryAction.label}</span>
								</span>
							</button>
							{#if d.secondaryAction}
								<button
									type="button"
									disabled={busyId === d.id}
									onclick={() => void act(d, 'secondary')}
									class="inbox-outline inline-flex h-9 items-center gap-1.5 rounded-lg px-3 text-xs font-semibold"
								>
									<Ban class="size-3.5" aria-hidden="true" />
									<span>{d.secondaryAction.label}</span>
								</button>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
{:else}
	<InboxView organizationId={data.connection.organization.id} />
{/if}

<style>
	.inbox-tab {
		color: var(--exec-muted);
	}
	.inbox-tab.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.inbox-badge {
		padding: 2px 9px;
		border-radius: 999px;
		font-weight: 600;
		background: var(--exec-sky-bg);
		color: var(--exec-sky);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
	}
	.inbox-badge.is-urgent {
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
	}
	.inbox-outline {
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.inbox-ok-ring {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
</style>
