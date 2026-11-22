<script lang="ts">
	import { onDestroy, tick } from 'svelte';
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
	import { inboxApi, type InboxItem } from '$lib/modules/inbox';
	import { executeDecisionAction } from '$lib/home/actions';
	import { deriveDecisionSpotlight, isDenyAction, type SpotlightDecision } from '$lib/home/model';
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/office/api';
	import { owner } from '$lib/org/model';
	import { workApi } from '$lib/work';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const ownerId = $derived(owner(snapshot)?.id ?? 'owner');

	let items = $state.raw<WorkItemSummary[] | undefined>(undefined);
	let inboxItems = $state.raw<InboxItem[]>([]);
	let filter = $state<'all' | 'approval' | 'decision' | 'problem' | 'work'>('all');
	let busyId = $state<string | undefined>(undefined);
	let decliningId = $state<string | undefined>(undefined);
	let declineReason = $state('');
	let denyInputEl = $state<HTMLInputElement | null>(null);
	let openContextIds = $state<Record<string, boolean>>({});
	let nowMs = $state(Date.now());

	const allDecisions = $derived(
		deriveDecisionSpotlight(items ?? [], snapshot, ownerId, nowMs, inboxItems)
	);
	const filtered = $derived.by(() => {
		if (filter === 'all') return allDecisions;
		if (filter === 'work') return allDecisions.filter((d) => d.sourceKind === 'work');
		return allDecisions.filter((d) => d.category === filter);
	});

	let loadSeq = 0;
	let destroyed = false;

	async function load(targetOrgId = organizationId, isCancelled: () => boolean = () => false) {
		const seq = ++loadSeq;
		const [workRes, inboxRes] = await Promise.allSettled([
			workApi.snapshot(targetOrgId),
			inboxApi.list(targetOrgId)
		]);
		if (destroyed || isCancelled() || seq !== loadSeq) return;
		if (workRes.status === 'fulfilled') {
			items = workRes.value.items;
		} else {
			const err = toAppError(workRes.reason);
			if (err.source === 'api' && (err.code === 'unsupported' || err.code === 'unavailable')) {
				items = [];
			} else {
				toast.error(err.message);
				items = [];
			}
		}
		inboxItems = inboxRes.status === 'fulfilled' ? inboxRes.value : [];
		nowMs = Date.now();
	}

	$effect(() => {
		const id = organizationId;
		let cancelled = false;
		items = undefined;
		inboxItems = [];
		decliningId = undefined;
		declineReason = '';
		void load(id, () => cancelled);
		return () => {
			cancelled = true;
		};
	});

	onDestroy(() => {
		destroyed = true;
	});

	async function armDeny(id: string) {
		decliningId = id;
		declineReason = '';
		await tick();
		denyInputEl?.focus();
	}

	async function cancelDeny() {
		const prevId = decliningId;
		decliningId = undefined;
		declineReason = '';
		await tick();
		if (prevId) document.getElementById(`inbox-deny-btn-${prevId}`)?.focus();
	}

	function handleDenyKeydown(event: KeyboardEvent, decision: SpotlightDecision) {
		if (event.key === 'Enter') {
			event.preventDefault();
			void act(decision, 'secondary');
		} else if (event.key === 'Escape') {
			event.preventDefault();
			void cancelDeny();
		}
	}

	function toggleContext(id: string) {
		openContextIds = { ...openContextIds, [id]: !openContextIds[id] };
	}

	async function act(decision: SpotlightDecision, kind: 'primary' | 'secondary') {
		if (busyId) return;
		if (
			kind === 'secondary' &&
			isDenyAction(decision.secondaryAction) &&
			decliningId !== decision.id
		) {
			await armDeny(decision.id);
			return;
		}
		busyId = decision.id;
		try {
			const res = await executeDecisionAction(
				organizationId,
				decision,
				kind,
				snapshot,
				declineReason
			);
			decliningId = undefined;
			declineReason = '';
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

<div class="inbox-page mx-auto flex w-full max-w-[1100px] flex-col gap-5 px-6 pt-3 pb-12 sm:px-8">
	<div class="inbox-head flex flex-wrap items-center justify-between gap-4">
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
			class="exec-well inline-flex flex-wrap items-center gap-1 p-1"
			role="tablist"
			aria-label="Filter decisions"
		>
			{#each [['all', 'All'], ['approval', 'Approvals'], ['decision', 'Decisions'], ['problem', 'Problems'], ['work', 'Work']] as [val, label] (val)}
				<button
					type="button"
					role="tab"
					aria-selected={filter === val}
					onclick={() => (filter = val as typeof filter)}
					class={[
						'inbox-tab rounded-lg px-3 py-1.5 text-xs font-semibold',
						filter === val && 'is-on'
					]}
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
				Your agents and teams will surface approvals, verifications, and blockers here when they
				need you.
			</p>
			<a href={resolve('/home')} class="exec-cta mt-2">
				<span class="exec-cta-fill" aria-hidden="true"></span>
				<span class="exec-cta-inner"><span>Back to Home</span></span>
			</a>
		</div>
	{:else}
		<ul class="inbox-list flex flex-col gap-3.5">
			{#each filtered as d (d.id)}
				{@const ctxOpen = Boolean(openContextIds[d.id])}
				<li class="exec-card inbox-card flex flex-col gap-3.5 p-5">
					<div class="flex flex-wrap items-center justify-between gap-2">
						<div
							class="flex flex-wrap items-center gap-2 text-xs"
							style="color: var(--exec-muted);"
						>
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
							<span>Details</span>
							<ChevronRight class="size-3.5" aria-hidden="true" />
						</a>
					</div>

					<h2 class="text-base font-semibold" style="color: var(--exec-ink);">{d.title}</h2>
					<p class="text-sm" style="color: var(--exec-body);">{d.recommendation}</p>

					<div class="exec-well grid grid-cols-1 gap-2 p-2.5 sm:grid-cols-3">
						{#each [d.risk, d.impact, d.proof] as cell, i (cell.label + i)}
							<div class="flex items-center gap-2 px-1.5">
								{#if cell.icon === 'shield-check'}
									<ShieldCheck
										class="size-4 shrink-0"
										style="color: var(--exec-ok);"
										aria-hidden="true"
									/>
								{:else if cell.icon === 'shield-alert'}
									<ShieldAlert
										class="size-4 shrink-0"
										style="color: var(--exec-warn);"
										aria-hidden="true"
									/>
								{:else if cell.icon === 'bolt'}
									<Zap class="size-4 shrink-0" style="color: var(--exec-sky);" aria-hidden="true" />
								{:else if cell.icon === 'scale'}
									<Scale
										class="size-4 shrink-0"
										style="color: var(--exec-sky);"
										aria-hidden="true"
									/>
								{:else if cell.icon === 'check-circle'}
									<CheckCircle2
										class="size-4 shrink-0"
										style="color: var(--exec-ok);"
										aria-hidden="true"
									/>
								{:else}
									<Clock
										class="size-4 shrink-0"
										style="color: var(--exec-muted);"
										aria-hidden="true"
									/>
								{/if}
								<div class="min-w-0">
									<div class="truncate text-xs font-semibold" style="color: var(--exec-ink);">
										{cell.label}
									</div>
									<div class="truncate text-[11px]" style="color: var(--exec-muted);">
										{cell.sub}
									</div>
								</div>
							</div>
						{/each}
					</div>

					<div class="flex flex-col gap-2">
						<button
							type="button"
							onclick={() => toggleContext(d.id)}
							aria-expanded={ctxOpen}
							aria-controls="inbox-ctx-{d.id}"
							class="inbox-ctx-toggle inline-flex w-fit items-center gap-1.5 rounded-md px-1.5 text-xs font-semibold"
						>
							<ChevronRight
								class={['size-3.5 transition-transform', ctxOpen && 'rotate-90']}
								aria-hidden="true"
							/>
							<span>{ctxOpen ? 'Hide background context' : 'Read background context'}</span>
						</button>
						{#if ctxOpen}
							<div
								id="inbox-ctx-{d.id}"
								class="inbox-ctx-body rounded-lg px-3.5 py-2.5 text-xs leading-relaxed"
							>
								<p>{d.backgroundContext}</p>
								{#if d.auditChecks.length > 0}
									<div
										class="inbox-ctx-checks mt-2.5 flex flex-wrap gap-x-3.5 gap-y-1.5 pt-2.5"
										role="list"
										aria-label="Decision checks"
									>
										{#each d.auditChecks as chk (chk)}
											<span class="inline-flex items-center gap-1.5" role="listitem">
												<CheckCircle2
													class="size-3.5 shrink-0"
													style="color: var(--exec-ok);"
													aria-hidden="true"
												/>
												<span>{chk}</span>
											</span>
										{/each}
									</div>
								{/if}
							</div>
						{/if}
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
						{#if isDenyAction(d.secondaryAction)}
							{#if decliningId !== d.id}
								<button
									id="inbox-deny-btn-{d.id}"
									type="button"
									disabled={busyId === d.id}
									onclick={() => void armDeny(d.id)}
									class="inbox-deny inline-flex h-9 items-center gap-1.5 rounded-lg px-3 text-xs font-semibold"
								>
									<Ban class="size-3.5" aria-hidden="true" />
									<span>{d.secondaryAction.label}</span>
								</button>
							{/if}
						{:else if d.secondaryAction}
							<button
								type="button"
								disabled={busyId === d.id}
								onclick={() => void act(d, 'secondary')}
								class="inbox-outline inline-flex h-9 items-center gap-1.5 rounded-lg px-3 text-xs font-semibold"
							>
								<span>{d.secondaryAction.label}</span>
							</button>
						{/if}

						{#if decliningId === d.id}
							<div
								class="inbox-deny-guard flex w-full flex-col gap-2 rounded-lg p-3"
								role="group"
								aria-label="Confirm deny"
							>
								<div class="flex flex-wrap items-center justify-between gap-2.5">
									<span
										class="inbox-deny-guard-lbl inline-flex items-center gap-1.5 text-xs font-semibold"
									>
										<Ban class="size-3.5 shrink-0" aria-hidden="true" />
										<span>Confirm deny?</span>
									</span>
									<div class="flex items-center gap-1.5">
										<button
											type="button"
											disabled={busyId === d.id}
											onclick={() => void act(d, 'secondary')}
											class="inbox-deny-confirm inline-flex h-8 items-center rounded-md px-3 text-xs font-semibold"
										>
											Confirm
										</button>
										<button
											type="button"
											onclick={() => void cancelDeny()}
											class="inbox-cancel inline-flex h-8 items-center rounded-md px-2.5 text-xs font-semibold"
										>
											Keep
										</button>
									</div>
								</div>
								<label class="sr-only" for="inbox-deny-msg-{d.id}">
									Optional reason for {d.actorName}
								</label>
								<input
									bind:this={denyInputEl}
									bind:value={declineReason}
									onkeydown={(e) => handleDenyKeydown(e, d)}
									id="inbox-deny-msg-{d.id}"
									type="text"
									autocomplete="off"
									placeholder="Optional reason for {d.actorName}… (Enter to confirm, Esc to cancel)"
									class="inbox-input min-h-9 w-full rounded-md px-2.5 py-1.5 text-xs"
								/>
							</div>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>

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
	.inbox-deny {
		background: var(--exec-deny-bg);
		color: var(--exec-deny-ink);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring);
	}
	.inbox-deny:hover {
		background: var(--exec-deny-bg-hover);
		color: var(--exec-deny-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring-hover);
	}
	.inbox-deny-guard {
		background: var(--exec-deny-guard-bg);
		box-shadow: inset 0 0 0 1px var(--exec-deny-guard-ring);
	}
	.inbox-deny-guard-lbl {
		color: var(--exec-deny-ink);
	}
	.inbox-deny-confirm {
		background: var(--exec-deny-confirm-bg);
		color: var(--exec-deny-confirm-ink);
	}
	.inbox-deny-confirm:hover {
		background: var(--exec-deny-confirm-hover);
	}
	.inbox-cancel {
		color: var(--exec-muted);
	}
	.inbox-cancel:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.inbox-input {
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.inbox-ctx-toggle {
		min-height: 32px;
		color: var(--exec-accent-ink);
	}
	.inbox-ctx-toggle:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.inbox-ctx-body {
		background: var(--exec-well);
		color: var(--exec-body);
		box-shadow: inset 2px 0 0 var(--tone-accent);
	}
	.inbox-ctx-checks {
		border-top: 1px solid var(--exec-border);
		color: var(--exec-muted);
	}
	.inbox-ok-ring {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
</style>
