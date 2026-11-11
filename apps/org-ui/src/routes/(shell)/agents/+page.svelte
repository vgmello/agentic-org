<script lang="ts">
	import { onDestroy } from 'svelte';
	import { afterNavigate, beforeNavigate, goto, invalidate } from '$app/navigation';
	import { navigating, page } from '$app/state';
	import AgentStatusIcon from '$lib/components/AgentStatusIcon.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import UserPlus from '@lucide/svelte/icons/user-plus';
	import { toast } from 'svelte-sonner';
	import { AGENTS_QUERY_LIMIT, AddAgentSheet, consumeAgentsAddIntent, normalizeAgentsQuery } from '$lib/modules/employees';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { filterRoster, roster, runModeLabel, runtimeLabel, type RosterFilter } from '$lib/org/agents';
	import { orgApi } from '$lib/org/api';
	import { withoutFormer } from '$lib/org/model';
	import { sessionFor } from '$lib/org/session';

	let { data } = $props();
	const connection = $derived(data.connection);
	const full = $derived(data.snapshot);
	const offices = $derived(connection.organization.offices);
	const rows = $derived(roster(full, offices));
	let filter = $state<RosterFilter>('all');
	let searchText = $state(normalizeAgentsQuery(page.url.searchParams.get('q') ?? ''));
	const query = $derived(normalizeAgentsQuery(searchText));
	const needle = $derived(query.toLowerCase());
	const shown = $derived(filterRoster(rows, filter).filter((row) =>
		row.member.name.toLowerCase().includes(needle) || (row.member.agent?.title ?? '').toLowerCase().includes(needle)
	));
	const counts = $derived({
		all: filterRoster(rows, 'all').length,
		active: filterRoster(rows, 'active').length,
		paused: filterRoster(rows, 'paused').length,
		left: filterRoster(rows, 'left').length
	});
	const FILTERS: { id: RosterFilter; label: string }[] = [
		{ id: 'all', label: 'All' },
		{ id: 'active', label: 'Active' },
		{ id: 'paused', label: 'Paused' },
		{ id: 'left', label: 'Former' }
	];

	const session = $derived(sessionFor(orgApi, connection.organization.id));
	let adding = $state(false);
	let addButton = $state<HTMLButtonElement | null>(null);
	let searchInput = $state<HTMLInputElement | null>(null);
	let openedFromUrl = $state(false);
	// A mounted route already has its loaded/available roster; extend this value when per-operation Add capabilities land.
	// Intent gating is defensive: afterNavigate has settled and pending navigation cancels its task, so canAdd is true there today.
	const canAdd = $derived(!navigating.to);
	function openAdd() {
		openedFromUrl = false;
		adding = true;
	}
	function restoreAddFocus(event: Event) {
		if (!openedFromUrl) return;
		event.preventDefault();
		(canAdd ? addButton : searchInput)?.focus();
	}

	function changeQuery(value: string) {
		searchText = value;
		const next = new URL(page.url);
		const q = normalizeAgentsQuery(value);
		if (q) next.searchParams.set('q', q);
		else next.searchParams.delete('q');
		if (next.href !== page.url.href) void goto(next, { replaceState: true, keepFocus: true, noScroll: true });
	}

	async function handleUrl() {
		const incoming = normalizeAgentsQuery(page.url.searchParams.get('q') ?? '');
		// Keep an in-progress trailing space so typing a multiword query remains possible.
		if (normalizeAgentsQuery(searchText) !== incoming) searchText = incoming;
		const intent = consumeAgentsAddIntent(page.url, { canAdd, alreadyOpen: adding });
		if (!intent) return;
		if (intent.open) {
			openedFromUrl = true;
			adding = true;
		}
		// Navigation's replaceState option keeps SvelteKit's URL/history metadata in sync too.
		await goto(intent.url, { replaceState: true, keepFocus: true, noScroll: true });
	}
	let navigationTask: ReturnType<typeof setTimeout> | undefined;
	beforeNavigate(() => clearTimeout(navigationTask));
	onDestroy(() => clearTimeout(navigationTask));
	afterNavigate(() => {
		clearTimeout(navigationTask);
		// Kit restores fragment focus in a task; consume after it so it cannot restore the old intent URL.
		navigationTask = setTimeout(handleUrl, 0);
	});
	async function added(r: { memberId: string; name: string; problems: string[] }) {
		await invalidate('app:org');
		if (r.problems.length === 0) toast.success(`Added ${r.name}. Everyone in ${connection.organization.name} can reach ${r.name}.`);
		else toast.warning(`Added ${r.name}, but not everything went through.`, { description: r.problems.join(' ') });
	}
</script>

<svelte:head><title>Agents · Agentic Org</title></svelte:head>

<div class="space-y-5 px-8 py-6">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div class="space-y-1">
			<p class="text-sm/6 text-gray-400">The AI employees of {connection.organization.name}: what each runs on, how it runs and where it works.</p>
			{#if data.sample}
				<p class="flex items-center gap-x-2 text-xs/5 text-gray-400" data-testid="sample-data-note"><Badge variant="secondary">Sample data</Badge> An invented organization, not yours.</p>
			{/if}
		</div>
		<div class="flex items-center gap-x-2">
			<Button bind:ref={addButton} disabled={!canAdd} onclick={openAdd}><Plus aria-hidden="true" /> Add agent</Button>
		</div>
	</div>
	<div class="max-w-md space-y-2">
		<Label for="agents-query" class="block text-sm/6 font-medium text-white">Search agents</Label>
		<Input bind:ref={searchInput} id="agents-query" type="search" maxlength={AGENTS_QUERY_LIMIT} value={searchText} aria-describedby="agents-query-hint" class="border-0 bg-white/5 px-3 py-1.5 text-base text-white shadow-none outline-1 outline-solid -outline-offset-1 outline-gray-500 sm:text-sm/6 md:text-sm/6 dark:bg-white/5 placeholder:text-gray-400 focus-visible:ring-0 focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-indigo-400" oninput={(event) => changeQuery(event.currentTarget.value)} onblur={() => (searchText = normalizeAgentsQuery(searchText))} />
		<p id="agents-query-hint" class="text-xs/5 text-gray-400">Name or role title</p>
	</div>

	{#if rows.length === 0}
		<div class="flex flex-col items-center px-8 py-16 text-center" data-testid="no-agents">
			<span class="flex size-12 items-center justify-center rounded-full bg-white/5 text-gray-300"><UserPlus class="size-6" aria-hidden="true" /></span>
			<h2 class="mt-4 text-base/7 font-semibold text-white">No agents yet</h2>
			<p class="mt-1 max-w-sm text-sm/6 text-gray-400">Agents are the AI employees of your organization. Add the first one to start giving them work.</p>
			<div class="mt-6 flex flex-wrap justify-center gap-3">
				<Button disabled={!canAdd} onclick={openAdd}>Add your first agent</Button>
			</div>
		</div>
	{:else}
		<div role="group" aria-label="Show agents" class="inline-flex rounded-lg bg-white/5 p-1 outline-1 -outline-offset-1 outline-white/10">
			{#each FILTERS as f (f.id)}
				<button
					type="button"
					aria-pressed={filter === f.id}
					onclick={() => (filter = f.id)}
					class="rounded-md px-3 py-1.5 text-sm/6 font-semibold focus-visible:outline-2 focus-visible:outline-indigo-400 {filter === f.id ? 'bg-white/10 text-white' : 'text-gray-400 hover:text-white'}"
				>
					{f.label} <span class="font-normal text-gray-400">{counts[f.id]}</span>
				</button>
			{/each}
		</div>

		{#if shown.length === 0}
			<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-gray-400 outline-1 -outline-offset-1 outline-white/10" data-testid="no-match">{query ? 'No agents match your search.' : `No ${filter} agents.`}</p>
		{:else}
			<div class="overflow-x-auto border-y border-white/5 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-indigo-400 focus-visible:-outline-offset-2">
				<table class="w-full text-left text-sm/6" data-testid="agent-roster">
					<caption class="sr-only">Agents</caption>
					<thead class="text-xs/5 text-gray-400">
						<tr>
							<th scope="col" class="px-4 py-2 font-medium">Agent</th>
							<th scope="col" class="px-4 py-2 font-medium">Runs on</th>
							<th scope="col" class="px-4 py-2 font-medium">How it runs</th>
							<th scope="col" class="px-4 py-2 font-medium">Status</th>
							<th scope="col" class="px-4 py-2 font-medium">Now</th>
							<th scope="col" class="px-4 py-2 font-medium">Teams</th>
							<th scope="col" class="px-4 py-2 font-medium">Office</th>
							<th scope="col" class="px-4 py-2 font-medium">Reports to</th>
						</tr>
					</thead>
					<tbody class="divide-y divide-white/5 text-gray-200">
						{#each shown as row (row.member.id)}
							<tr data-testid="agent-row" data-agent={row.member.id}>
								<th scope="row" class="px-4 py-4 font-normal">
									<a href="/org-chart?member={row.member.id}" class="block font-semibold text-white hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]">{row.member.name}</a>
									<span class="block text-xs/5 text-gray-400">{row.member.agent?.title ?? ''} · <code class="font-mono">{row.member.agent?.messaging_name ?? ''}</code></span>
								</th>
								<td class="px-4 py-4">{runtimeLabel(row.member.agent?.runtime)}</td>
								<td class="px-4 py-4">{runModeLabel(row.member.agent?.run_mode)}</td>
								<td class="px-4 py-4">
									<StatusPill label={row.live.word + (row.live.detail ? ` · ${row.live.detail}` : '')} class={row.live.id === 'paused' ? 'bg-amber-400/10 text-amber-200 ring-amber-400/30' : ''} data-testid="agent-status">
										{#snippet icon()}<AgentStatusIcon status={row.live.id} />{/snippet}
									</StatusPill>
								</td>
								<!-- What it is doing now comes with the Activity feed; until then there is nothing to say. -->
								<td class="px-4 py-4 text-gray-400" data-testid="agent-now" aria-label="Nothing reported">—</td>
								<td class="px-4 py-4 [overflow-wrap:anywhere]">{row.teams.length > 0 ? row.teams.join(', ') : '—'}</td>
								<td class="px-4 py-4">{row.office}</td>
								<td class="px-4 py-4 [overflow-wrap:anywhere]">{row.manager}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	{/if}
</div>

<AddAgentSheet bind:open={adding} snapshot={withoutFormer(full)} fullSnapshot={full} {session} officeId={connection.office.id} officeName={connection.office.name} organization={connection.organization.name} ondone={added} onCloseAutoFocus={restoreAddFocus} />
