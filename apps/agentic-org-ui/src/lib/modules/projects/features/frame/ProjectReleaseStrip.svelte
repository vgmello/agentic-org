<!--
  @component
  Project Release Readiness Strip (Prototype Release 53728c05):
  Sits immediately beneath the project toolbar on Project Overview and Project Activity (Assembly & Feed).
  Reads real release records (when served), live project work items, and project-scoped inbox gates;
  never fabricates fake releases or counts.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Ban from '@lucide/svelte/icons/ban';
	import Check from '@lucide/svelte/icons/check';
	import Clock from '@lucide/svelte/icons/clock';
	import { Resource } from '$lib/components/resource.svelte';
	import { isItemDone } from '$lib/home/model';
	import { releasesApi } from '$lib/modules/releases/api';
	import type { ReleaseDetail, ReleaseRow } from '$lib/modules/releases/types';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import { countedTasks, projectHref } from '../../present';
	import type { TaskCounts } from '../../types';

	let {
		organizationId,
		projectId,
		projectName,
		fallbackTasks
	}: {
		organizationId: string;
		projectId: string;
		projectName: string;
		fallbackTasks: TaskCounts;
	} = $props();

	let shellData: ReturnType<typeof useShellData> | undefined;
	try {
		shellData = useShellData();
	} catch {
		shellData = undefined;
	}

	const releasesRead = new Resource(() => releasesApi.list(organizationId, projectId));

	const activeRelease = $derived.by<ReleaseRow | undefined>(() => {
		const list = releasesRead.data?.releases ?? [];
		if (list.length === 0) return undefined;
		return (
			list.find((r) => r.status === 'in_progress') ??
			list.find((r) => r.status === 'planned') ??
			list[0]
		);
	});

	let releaseDetail = $state.raw<ReleaseDetail | undefined>(undefined);
	let releaseDetailSeq = 0;

	$effect(() => {
		const orgId = organizationId;
		const projId = projectId;
		const relId = activeRelease?.id;
		const seq = ++releaseDetailSeq;
		if (!relId) {
			releaseDetail = undefined;
			return;
		}
		let cancelled = false;
		void releasesApi
			.get(orgId, projId, relId)
			.then((detail) => {
				if (cancelled || seq !== releaseDetailSeq) return;
				releaseDetail = detail;
			})
			.catch(() => {
				if (cancelled || seq !== releaseDetailSeq) return;
				releaseDetail = undefined;
			});
		return () => {
			cancelled = true;
		};
	});

	const releaseName = $derived.by(() => {
		if (!activeRelease) return 'Current delivery';
		const label = activeRelease.versionLabel?.trim();
		return label
			? `${label} · ${activeRelease.title}`
			: `R-${activeRelease.number} · ${activeRelease.title}`;
	});

	const shellReady = $derived(
		Boolean(shellData && shellData.loaded && shellData.loadedOrganizationId === organizationId)
	);

	const readiness = $derived.by(() => {
		// Prefer release-scoped task/milestone counts when the active release detail is available.
		if (releaseDetail) {
			const relTasks = releaseDetail.tasks.filter((t) => t.status !== 'cancelled');
			if (relTasks.length > 0) {
				const merged = relTasks.filter((t) => t.status === 'done' || t.status === 'closed').length;
				const total = relTasks.length;
				const pct =
					merged <= 0
						? 0
						: merged >= total
							? 100
							: Math.min(99, Math.round((merged * 100) / total));
				return { merged, total, pct, scopeLabel: 'Release readiness' };
			}
			const msDone = releaseDetail.milestones.reduce((sum, m) => sum + m.tasksDone, 0);
			const msOpen = releaseDetail.milestones.reduce((sum, m) => sum + m.tasksOpen, 0);
			const msTotal = msDone + msOpen;
			if (msTotal > 0) {
				const pct =
					msDone <= 0
						? 0
						: msDone >= msTotal
							? 100
							: Math.min(99, Math.round((msDone * 100) / msTotal));
				return { merged: msDone, total: msTotal, pct, scopeLabel: 'Release readiness' };
			}
		}
		if (shellReady && shellData && !shellData.workError) {
			const items = shellData.workItems.filter(
				(item) =>
					!item.deleted &&
					item.project_id === projectId &&
					!(item.kind === 'request' && item.status === 'declined') &&
					!(item.kind === 'task' && (item.resolution === 'cancelled' || item.status === 'closed'))
			);
			if (items.length > 0) {
				const merged = items.filter((item) => isItemDone(item)).length;
				const total = items.length;
				const pct =
					merged <= 0
						? 0
						: merged >= total
							? 100
							: Math.min(99, Math.round((merged * 100) / total));
				return { merged, total, pct, scopeLabel: 'Project readiness' };
			}
		}
		const total = countedTasks(fallbackTasks);
		const merged = fallbackTasks.done;
		const pct =
			total <= 0 || merged <= 0
				? 0
				: merged >= total
					? 100
					: Math.min(99, Math.round((merged * 100) / total));
		return { merged, total, pct, scopeLabel: 'Project readiness' };
	});

	interface GatePill {
		id: string;
		label: string;
		state: 'pending' | 'blocked' | 'cleared';
		status: string;
		href: string;
	}

	const gatePills = $derived.by<GatePill[]>(() => {
		if (!shellReady || !shellData) return [];
		const scoped = shellData.inboxItems.filter(
			(item) =>
				item.project?.id === projectId && (item.kind === 'approval' || item.kind === 'decision')
		);
		return scoped.slice(0, 4).map((item) => {
			if (item.state !== 'handled') {
				return {
					id: item.id,
					label: item.title,
					state: 'pending' as const,
					status: 'Pending',
					href: `/inbox?item=${encodeURIComponent(item.id)}`
				};
			}
			const outcome = item.handled?.outcome ?? 'Handled';
			const blocked = /^(denied|declined|dismissed|changes requested)\b/i.test(outcome);
			return {
				id: item.id,
				label: item.title,
				state: blocked ? ('blocked' as const) : ('cleared' as const),
				status: blocked ? 'Blocked' : 'Cleared',
				href: `/inbox?item=${encodeURIComponent(item.id)}`
			};
		});
	});

	const releaseHref = $derived(
		activeRelease
			? `${projectHref(projectId, 'releases')}/${encodeURIComponent(activeRelease.id)}`
			: undefined
	);
</script>

<section
	class="assembly-release"
	id="project-release-summary"
	data-testid="project-release-strip"
	aria-label="Release readiness for {projectName}"
>
	<div class="assembly-release-overview">
		<div class="min-w-0">
			<p class="assembly-release-label">Release</p>
			{#if releaseHref}
				<a href={resolve(releaseHref as '/')} class="assembly-release-name assembly-release-link">
					{releaseName}
				</a>
			{:else}
				<p class="assembly-release-name">{releaseName}</p>
			{/if}
		</div>

		<div class="assembly-release-readiness">
			<p class="assembly-release-label" data-testid="project-release-readiness">
				{#if readiness.total > 0}
					{readiness.scopeLabel} {readiness.pct}% · {readiness.merged}/{readiness.total} merged
				{:else}
					No delivery items recorded yet
				{/if}
			</p>
			<progress
				class={[
					'assembly-progress-bar',
					readiness.total > 0 && readiness.merged >= readiness.total && 'is-complete'
				]}
				value={readiness.merged}
				max={Math.max(1, readiness.total)}
				aria-label="{readiness.scopeLabel}: {readiness.pct}%"
			></progress>
		</div>
	</div>

	{#if gatePills.length > 0}
		<ul class="assembly-release-gates" role="list" aria-label="Release gates">
			{#each gatePills as gate (gate.id)}
				<li>
					<a
						href={resolve(gate.href as '/inbox')}
						data-gate-state={gate.state}
						title="{gate.label} · {gate.status}"
					>
						{#if gate.state === 'cleared'}
							<Check class="size-3.5 shrink-0" aria-hidden="true" />
						{:else if gate.state === 'blocked'}
							<Ban class="size-3.5 shrink-0" aria-hidden="true" />
						{:else}
							<Clock class="size-3.5 shrink-0" aria-hidden="true" />
						{/if}
						<span class="truncate">{gate.label}</span>
						<span class="sr-only"> · {gate.status}</span>
					</a>
				</li>
			{/each}
		</ul>
	{:else if releaseHref}
		<ul class="assembly-release-gates" role="list" aria-label="Release status">
			<li>
				<a href={resolve(releaseHref as '/')} data-gate-state="cleared">
					<Check class="size-3.5 shrink-0" aria-hidden="true" />
					<span>Release details</span>
				</a>
			</li>
		</ul>
	{/if}
</section>

<style>
	.assembly-release {
		display: flex;
		flex: none;
		flex-wrap: wrap;
		align-items: center;
		gap: 12px 22px;
		padding: 14px 1rem;
		border-block-end: 1px solid var(--line);
		background: var(--detail-panel-surface, var(--surface-sunken, var(--exec-bg)));
	}

	@media (min-width: 640px) {
		.assembly-release {
			padding-inline: 1.5rem;
		}
	}

	@media (min-width: 1024px) {
		.assembly-release {
			padding-inline: 2rem;
		}
	}

	.assembly-release-overview {
		display: flex;
		flex: 1 1 240px;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 24px;
		min-width: 0;
	}

	.assembly-release-label,
	.assembly-release-name {
		font-variant-numeric: tabular-nums;
	}

	.assembly-release-label {
		margin: 0;
		color: var(--fg-muted);
		font-size: 0.75rem;
		line-height: 1.35;
	}

	.assembly-release-name {
		margin: 0;
		color: var(--fg);
		font-size: 0.9375rem;
		font-weight: 650;
		line-height: 1.35;
		overflow-wrap: anywhere;
	}

	.assembly-release-link {
		display: inline-block;
		border-radius: 4px;
		text-decoration: none;
	}

	.assembly-release-link:hover {
		color: var(--tone-accent-ink);
		text-decoration: underline;
		text-underline-offset: 2px;
	}

	.assembly-release-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	.assembly-release-readiness {
		display: grid;
		flex: 1 1 160px;
		gap: 5px;
		min-width: 0;
	}

	.assembly-progress-bar {
		width: 100%;
		height: 6px;
		appearance: none;
		border: 0;
		border-radius: 999px;
		overflow: hidden;
		background: var(--bar-track);
	}

	.assembly-progress-bar::-webkit-progress-bar {
		background: var(--bar-track);
		border-radius: 999px;
	}

	.assembly-progress-bar::-webkit-progress-value {
		background: var(--tone-accent-bar);
		border-radius: 999px;
	}

	.assembly-progress-bar.is-complete::-webkit-progress-value {
		background: var(--tone-ok-bar);
	}

	.assembly-progress-bar::-moz-progress-bar {
		background: var(--tone-accent-bar);
		border-radius: 999px;
	}

	.assembly-progress-bar.is-complete::-moz-progress-bar {
		background: var(--tone-ok-bar);
	}

	.assembly-release-gates {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		margin: 0;
		margin-inline-start: auto;
		padding: 0;
		list-style: none;
	}

	.assembly-release-gates a {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		min-height: 30px;
		max-width: 18rem;
		padding: 4px 10px;
		border: 1px solid color-mix(in srgb, currentColor 45%, transparent);
		border-radius: 999px;
		color: var(--tone-warn-ink);
		font-size: 0.75rem;
		font-weight: 550;
		text-decoration: none;
	}

	.assembly-release-gates a[data-gate-state='cleared'] {
		color: var(--tone-ok-ink);
	}

	.assembly-release-gates a[data-gate-state='blocked'] {
		color: var(--tone-bad-ink);
	}

	.assembly-release-gates a:hover {
		background: var(--surface-raised);
		border-color: currentColor;
	}

	.assembly-release-gates a:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	@media (max-width: 620px) {
		.assembly-release {
			padding: 10px 1rem;
		}
		.assembly-release-gates {
			margin-inline-start: 0;
		}
	}
</style>
