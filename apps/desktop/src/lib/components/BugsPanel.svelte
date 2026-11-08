<script lang="ts">
	import { onMount } from 'svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';
	import BugSheet from '$lib/components/BugSheet.svelte';
	import ReportBugDialog from '$lib/components/ReportBugDialog.svelte';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/office/api';
	import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { sender, workApi } from '$lib/work';
	import { generalAreaId } from '$lib/work/api';
	import { BUG_STATUS_LABEL, bugRef, closedWord, FILTERS, filterBugs, groupByArea, severityWord, type BugFilter } from '$lib/work/bugs';
	import SeverityIcon from '$lib/components/SeverityIcon.svelte';
	import { nameOf } from '$lib/work/people';

	/** A project's bugs (work-items-ux §6): grouped by area, filtered by where they are in their life. */
	let {
		organizationId,
		projectId,
		projectName,
		snapshot
	}: { organizationId: string; projectId: string; projectName: string; snapshot: OrgSnapshot } = $props();

	let all = $state<WorkItemSummary[] | undefined>();
	let loadError = $state<string | undefined>();
	let filter = $state<BugFilter>('open');
	let reporting = $state(false);
	let openId = $state<string | undefined>();
	let sheetOpen = $state(false);
	let announcement = $state('');
	let announced = 0;
	const announce = (text: string) => (announcement = text + (announced++ % 2 ? '​' : ''));

	// Until the area commands exist every project has just General.
	const areas = $derived([{ id: generalAreaId(projectId), name: 'General', general: true }]);
	const send = $derived(sender(organizationId));
	const bugs = $derived((all ?? []).filter((b) => b.kind === 'bug'));
	const shown = $derived(filterBugs(bugs, filter));
	const groups = $derived(groupByArea(shown, areas));
	const counts = $derived(Object.fromEntries(FILTERS.map((f) => [f.id, filterBugs(bugs, f.id).length])) as Record<BugFilter, number>);

	async function load() {
		try {
			all = (await workApi.snapshot(organizationId, projectId)).items;
			loadError = undefined;
		} catch (e) {
			if (all === undefined) loadError = toAppError(e).message;
			else toast.error(toAppError(e).message);
		}
	}
	onMount(load);
	const open = (id: string) => ((openId = id), (sheetOpen = true));
</script>

<div class="space-y-4">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<p class="text-sm/6 text-gray-400">Bugs in {projectName}. <span class="rounded bg-white/10 px-1.5 text-xs/5 text-gray-200">Sample data</span></p>
		<Button onclick={() => (reporting = true)}><Plus aria-hidden="true" /> Report a bug</Button>
	</div>

	{#if loadError}
		<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-red-300" role="alert">{loadError} <button class="underline" onclick={load}>Try again</button></p>
	{:else if all === undefined}
		<p class="text-sm/6 text-gray-400">Loading bugs…</p>
	{:else}
		<div role="group" aria-label="Show bugs" class="inline-flex flex-wrap rounded-lg bg-white/5 p-1 outline-1 -outline-offset-1 outline-white/10">
			{#each FILTERS as f (f.id)}
				<button type="button" aria-pressed={filter === f.id} onclick={() => (filter = f.id)} class="rounded-md px-3 py-1.5 text-sm/6 font-semibold focus-visible:outline-2 focus-visible:outline-indigo-400 {filter === f.id ? 'bg-white/10 text-white' : 'text-gray-400 hover:text-white'}">
					{f.label} <span class="font-normal text-gray-400">{counts[f.id]}</span>
				</button>
			{/each}
		</div>

		{#if bugs.length === 0}
			<p class="text-sm/6 text-gray-400" data-testid="no-bugs">No bugs reported yet.</p>
		{:else if shown.length === 0}
			<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-gray-400" data-testid="no-match">Nothing here.</p>
		{:else}
			<div class="space-y-4" data-testid="bug-list">
				{#each groups as group (group.areaId)}
					<section aria-labelledby={`area-${group.areaId}`} class="overflow-hidden rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10">
						<h3 id={`area-${group.areaId}`} class="px-4 py-2 text-sm/6 font-semibold text-white">{group.name} <span class="font-normal text-gray-400">{group.bugs.length}</span></h3>
						<ul class="divide-y divide-white/5">
							{#each group.bugs as bug (bug.id)}
								<li class="flex flex-wrap items-center gap-x-3 px-4 py-2 text-sm/6" data-testid="bug-row" data-bug={bug.id}>
									<span class="text-gray-400">{bugRef(bug)}</span>
									<button type="button" class="min-w-0 flex-1 text-left font-semibold text-white hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]" onclick={() => open(bug.id)}>{bug.title}</button>
									<span class={bug.severity === 'critical' ? 'font-semibold text-amber-300' : 'text-gray-300'}><SeverityIcon severity={bug.severity} /> {severityWord(bug.severity)}{bug.severity_confirmed ? '' : '?'}</span>
									<span class="text-gray-300">{BUG_STATUS_LABEL[bug.status] ?? bug.status}{bug.status === 'closed' ? ` · ${closedWord(bug, bugs.find((b) => b.id === bug.duplicate_of))}` : ''}</span>
									<span class="text-gray-400">{nameOf(snapshot, bug.assignee_id)}</span>
								</li>
							{/each}
						</ul>
					</section>
				{/each}
			</div>
		{/if}
	{/if}
	<p class="sr-only" role="status" aria-live="polite">{announcement}</p>
</div>

<ReportBugDialog bind:open={reporting} {projectId} {projectName} {areas} {bugs} triager={nameOf(snapshot, snapshot.members.find((m) => m.owner)?.id)} {send} onopen={open} ondone={(created) => { toast.success(`Bug ${created.ref} reported.`); void load(); }} />
<BugSheet bind:open={sheetOpen} {organizationId} bugId={openId} {snapshot} {areas} {bugs} items={all ?? []} {send} onchanged={(message) => { if (message) announce(message); void load(); }} />
