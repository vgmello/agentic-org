<script lang="ts">
	import { invalidate } from '$app/navigation';
	import AgentStatusIcon from '$lib/components/AgentStatusIcon.svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import UserPlus from '@lucide/svelte/icons/user-plus';
	import { toast } from 'svelte-sonner';
	import { AddAgentSheet } from '$lib/modules/employees';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
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
	const shown = $derived(filterRoster(rows, filter));
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
			<Button onclick={() => (adding = true)}><Plus aria-hidden="true" /> Add agent</Button>
		</div>
	</div>

	{#if rows.length === 0}
		<div class="flex flex-col items-center px-8 py-16 text-center" data-testid="no-agents">
			<span class="flex size-12 items-center justify-center rounded-full bg-white/5 text-gray-300"><UserPlus class="size-6" aria-hidden="true" /></span>
			<h2 class="mt-4 text-base/7 font-semibold text-white">No agents yet</h2>
			<p class="mt-1 max-w-sm text-sm/6 text-gray-400">Agents are the AI employees of your organization. Add the first one to start giving them work.</p>
			<div class="mt-6 flex flex-wrap justify-center gap-3">
				<Button onclick={() => (adding = true)}>Add your first agent</Button>
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
			<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-gray-400 outline-1 -outline-offset-1 outline-white/10" data-testid="no-match">No {filter} agents.</p>
		{:else}
			<div class="overflow-x-auto rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10">
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
								<th scope="row" class="px-4 py-2 font-normal">
									<a href="/org-chart?member={row.member.id}" class="block font-semibold text-white hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]">{row.member.name}</a>
									<span class="block text-xs/5 text-gray-400">{row.member.agent?.title ?? ''} · <code class="font-mono">{row.member.agent?.messaging_name ?? ''}</code></span>
								</th>
								<td class="px-4 py-2">{runtimeLabel(row.member.agent?.runtime)}</td>
								<td class="px-4 py-2">{runModeLabel(row.member.agent?.run_mode)}</td>
								<td class="px-4 py-2">
									<span class="inline-flex items-center gap-x-1.5 {row.live.id === 'paused' ? 'text-amber-200' : ''}" data-testid="agent-status">
										<AgentStatusIcon status={row.live.id} />
										{row.live.word}{row.live.detail ? ` · ${row.live.detail}` : ''}
									</span>
								</td>
								<!-- What it is doing now comes with the Activity feed; until then there is nothing to say. -->
								<td class="px-4 py-2 text-gray-400" data-testid="agent-now" aria-label="Nothing reported">—</td>
								<td class="px-4 py-2 [overflow-wrap:anywhere]">{row.teams.length > 0 ? row.teams.join(', ') : '—'}</td>
								<td class="px-4 py-2">{row.office}</td>
								<td class="px-4 py-2 [overflow-wrap:anywhere]">{row.manager}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	{/if}
</div>

<AddAgentSheet bind:open={adding} snapshot={withoutFormer(full)} fullSnapshot={full} {session} officeId={connection.office.id} officeName={connection.office.name} organization={connection.organization.name} ondone={added} />
