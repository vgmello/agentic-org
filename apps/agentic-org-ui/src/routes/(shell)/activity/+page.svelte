<script lang="ts">
	import { resolve } from '$app/paths';
	import Activity from '@lucide/svelte/icons/activity';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Play from '@lucide/svelte/icons/play';
	import { isItemDone, relativeAge } from '$lib/home/model';
	import { activityApi } from '$lib/modules/activity/api';
	import {
		KIND_WORD,
		type ActivityKindFilter,
		type ActivityRow
	} from '$lib/modules/activity/types';
	import { pageTitle } from '$lib/nav';
	import { memberById } from '$lib/org/model';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const projectsById = $derived(new Map((snapshot.projects ?? []).map((p) => [p.id, p.name])));
	const shellData = useShellData();

	let activityRows = $state.raw<ActivityRow[] | undefined>(undefined);
	const workItems = $derived(
		shellData.loaded && shellData.loadedOrganizationId === organizationId
			? shellData.workItems
			: undefined
	);
	let kindFilter = $state<ActivityKindFilter>('all');
	let nowMs = $state(Date.now());

	interface UnifiedEntry {
		id: string;
		kind: ActivityRow['kind'];
		title: string;
		meta: string;
		preview?: string;
		at: string;
		tone: 'ok' | 'warn' | 'accent' | 'sky';
		href: string;
	}

	const entries = $derived.by<UnifiedEntry[]>(() => {
		if (activityRows && activityRows.length > 0) {
			const filtered =
				kindFilter === 'all' ? activityRows : activityRows.filter((r) => r.kind === kindFilter);
			return filtered.map((r) => ({
				id: r.id,
				kind: r.kind,
				title: `${r.actor.name} ${r.text}`,
				meta: [
					KIND_WORD[r.kind],
					...r.projectIds.map((pid) => projectsById.get(pid)).filter(Boolean)
				].join(' · '),
				preview: r.preview ?? r.run?.outcome ?? r.run?.trigger,
				at: r.at,
				tone:
					r.action === 'task-done' || r.action === 'merge' || r.run?.state === 'succeeded'
						? 'ok'
						: r.run?.state === 'failed' || r.kind === 'decision'
							? 'warn'
							: r.kind === 'conversation'
								? 'sky'
								: 'accent',
				href: r.href?.path ?? '/work'
			}));
		}

		const list = (workItems ?? []).filter((i) => !i.deleted);
		const mapped: UnifiedEntry[] = [...list]
			.sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at))
			.map((item) => {
				const actor =
					memberById(snapshot, item.assignee_id) ?? memberById(snapshot, item.created_by);
				const proj = item.project_id ? projectsById.get(item.project_id) : undefined;
				const done = isItemDone(item);
				return {
					id: item.id,
					kind: item.kind === 'request' ? 'decision' : 'work',
					title: `${item.title} (${item.short_id})`,
					meta: `${actor?.name ?? 'Team'} · ${item.status.replace('_', ' ')}${proj ? ` · ${proj}` : ''}`,
					at: item.updated_at,
					tone: done ? 'ok' : item.status === 'blocked' ? 'warn' : 'accent',
					href:
						item.kind === 'request'
							? `/work?request=${encodeURIComponent(item.id)}`
							: `/work/${encodeURIComponent(item.short_id)}`
				};
			});

		return kindFilter === 'all' ? mapped : mapped.filter((e) => e.kind === kindFilter);
	});

	const loading = $derived(
		activityRows === undefined || (activityRows.length === 0 && workItems === undefined)
	);

	let loadSeq = 0;

	$effect(() => {
		const id = organizationId;
		const seq = ++loadSeq;
		let cancelled = false;
		activityRows = undefined;
		void activityApi
			.feed(id, { kind: 'all' }, 40)
			.then((res) => {
				if (cancelled || seq !== loadSeq) return;
				activityRows = res.rows;
				nowMs = Date.now();
			})
			.catch(() => {
				if (cancelled || seq !== loadSeq) return;
				activityRows = [];
				nowMs = Date.now();
			});
		return () => {
			cancelled = true;
		};
	});
</script>

<svelte:head><title>{pageTitle('Activity')}</title></svelte:head>

<div class="act-page flex w-full flex-col gap-5 px-6 pt-3 pb-12 sm:px-8">
	<div class="act-head flex flex-wrap items-center justify-between gap-4">
		<div>
			<h1
				id="shell-page-title"
				tabindex="-1"
				class="text-2xl font-semibold tracking-tight focus:outline-none"
				style="color: var(--exec-ink);"
			>
				Activity
			</h1>
			<p class="mt-0.5 text-sm" style="color: var(--exec-muted);">
				Latest updates across work, runs, conversations, and decisions in {data.connection
					.organization.name}.
			</p>
		</div>
		<div
			class="exec-well inline-flex flex-wrap items-center gap-1 p-1"
			role="tablist"
			aria-label="Filter activity"
		>
			{#each [['all', 'All'], ['work', 'Work'], ['run', 'Runs'], ['decision', 'Decisions'], ['conversation', 'Chat'], ['org', 'Org']] as [val, label] (val)}
				<button
					type="button"
					role="tab"
					aria-selected={kindFilter === val}
					onclick={() => (kindFilter = val as ActivityKindFilter)}
					class={[
						'act-tab rounded-lg px-3 py-1.5 text-xs font-semibold',
						kindFilter === val && 'is-on'
					]}
				>
					{label}
				</button>
			{/each}
		</div>
	</div>

	{#if loading}
		<div class="exec-card p-6 text-sm" style="color: var(--exec-muted);">Loading activity…</div>
	{:else if entries.length === 0}
		<div class="exec-card flex flex-col items-center gap-2 px-6 py-16 text-center">
			<Activity class="size-8" style="color: var(--exec-muted);" aria-hidden="true" />
			<h2 class="text-base font-semibold" style="color: var(--exec-ink);">No activity yet</h2>
			<p class="max-w-sm text-sm" style="color: var(--exec-muted);">
				Work updates from your agents and teammates will appear here chronologically.
			</p>
		</div>
	{:else}
		<ul class="exec-card act-list divide-y divide-line-subtle overflow-hidden">
			{#each entries as entry (entry.id)}
				<li>
					<a
						href={resolve(entry.href as '/work')}
						class="flex items-start gap-3 px-5 py-3.5 transition-colors hover:bg-surface-raised"
					>
						<span class="mt-0.5 shrink-0">
							{#if entry.tone === 'ok'}
								<CheckCircle2 class="size-4" style="color: var(--exec-ok);" aria-hidden="true" />
							{:else if entry.tone === 'warn'}
								<AlertTriangle class="size-4" style="color: var(--exec-warn);" aria-hidden="true" />
							{:else if entry.kind === 'conversation'}
								<MessageSquare class="size-4" style="color: var(--exec-sky);" aria-hidden="true" />
							{:else if entry.kind === 'run'}
								<Play class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
							{:else}
								<Clock class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
							{/if}
						</span>
						<div class="min-w-0 flex-1">
							<div class="text-sm font-semibold" style="color: var(--exec-ink);">{entry.title}</div>
							<p class="mt-0.5 text-xs" style="color: var(--exec-muted);">{entry.meta}</p>
							{#if entry.preview}
								<p class="mt-1 text-xs" style="color: var(--exec-body);">{entry.preview}</p>
							{/if}
						</div>
						<span class="shrink-0 text-xs tabular-nums" style="color: var(--exec-dim);">
							{relativeAge(entry.at, nowMs)}
						</span>
						<ChevronRight
							class="mt-0.5 size-4 shrink-0"
							style="color: var(--exec-dim);"
							aria-hidden="true"
						/>
					</a>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.act-tab {
		color: var(--exec-muted);
	}
	.act-tab.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
</style>
