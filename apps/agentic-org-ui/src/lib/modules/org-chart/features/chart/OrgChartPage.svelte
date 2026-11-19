<!--
  @component
  Teams › Org chart (offices-and-teams-ux §7.1, §7.8, §7.10): who reports to whom. The owner bar on top, then one
  vertical reporting tree drawn from explicit reporting lines only, under "No explicit manager". Every active
  employee appears once. Actions: Add a person and Hire an agent in the header; Change manager… and Move home team…
  in each node's menu; Hand over ownership… on the owner bar for the owner. No zoom, no chart search, no team canvas:
  team hierarchy lives in the Teams index.
  States: loading (shaped like the page), unavailable (said in place), error with Retry, empty, ready. Live changes
  (`invalidate('app:org')`, counted by the route as `loads`) re-read in place, keeping the shown tree, what is
  collapsed and focus.
-->
<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { SvelteSet } from 'svelte/reactivity';
	import { toast } from 'svelte-sonner';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { hireUrl } from '$lib/modules/employees';
	import { MoveHomeTeamDialog } from '$lib/modules/members';
	import { AddPersonToTeam } from '$lib/modules/teams';
	import type { Connection, OrgSnapshot } from '$lib/proto';
	import { orgChartApi } from '../../api';
	import { TREE_KEYS, treeMove } from '../../keyboard';
	import {
		ancestorsOf,
		handOverCandidates,
		initiallyCollapsed,
		managerChoices,
		reportingTree,
		topLevelTeam
	} from '../../tree';
	import type { ChartEmployee } from '../../types';
	import ChangeManagerDialog from '../change-manager/ChangeManagerDialog.svelte';
	import HandOverDialog from '../owner/HandOverDialog.svelte';
	import OwnerBar from '../owner/OwnerBar.svelte';
	import ChartNode, { type NodeAction } from './ChartNode.svelte';
	import ChartSkeleton from './ChartSkeleton.svelte';

	let {
		connection,
		snapshot,
		loads
	}: {
		connection: Connection;
		/**
		 * The team tree and people, for Add a person and Move home team. `null` while the server doesn't serve it:
		 * those two are then left out rather than offered without their choices.
		 */
		snapshot: OrgSnapshot | null;
		/** The route's load count: a change means the organization changed, so re-read the chart. */
		loads: number;
	} = $props();

	const organizationId = $derived(connection.organization.id);
	const collapsed = new SvelteSet<string>();
	let seeded = false;
	const chart = new Resource(async () => {
		const read = await orgChartApi.chart(organizationId);
		// A large organization starts with levels below two collapsed (§7.10); later reads keep the person's choice.
		if (!seeded) {
			seeded = true;
			for (const id of initiallyCollapsed(reportingTree(read.employees))) collapsed.add(id);
		}
		return read;
	});

	// Changes made elsewhere (New › Add a person, a hire, a live office change) invalidate the route's org data: re-read
	// then, keeping the shown tree until the answer arrives. The first load is the Resource's own.
	let seenLoad = untrack(() => loads);
	$effect(() => {
		const load = loads;
		if (load === seenLoad) return;
		seenLoad = load;
		untrack(() => void chart.reload());
	});

	const tree = $derived(chart.data ? reportingTree(chart.data.employees) : undefined);
	const owner = $derived(chart.data?.employees.find((e) => e.id === chart.data?.ownerId));
	const viewerOwns = $derived(!!chart.data?.viewerId && chart.data.viewerId === chart.data.ownerId);
	const roots = $derived(tree?.roots.length ?? 0);
	/** Add a person starts at the top-level team the owner's home team sits in (§7.4), else the first top team. */
	const topTeamId = $derived(
		snapshot ? topLevelTeam(snapshot.teams, owner?.homeTeam?.id) : undefined
	);

	// ---- dialogs: one at a time, each mounted fresh per opening ----
	type Open =
		{ kind: 'manager' | 'move'; employeeId: string; n: number } | { kind: 'handover'; n: number };
	let opened = $state<Open | undefined>();
	let dialogOpen = $state(false);
	let openings = 0;
	const target = $derived.by(() => {
		const o = opened;
		return o && o.kind !== 'handover'
			? chart.data?.employees.find((e) => e.id === o.employeeId)
			: undefined;
	});

	function onaction(action: NodeAction, employee: ChartEmployee) {
		opened = { kind: action, employeeId: employee.id, n: ++openings };
		dialogOpen = true;
	}

	/** Focus a node's control once it is drawn, opening the managers above it first. */
	async function focusNode(id: string, part: 'link' | 'menu') {
		if (tree) for (const up of ancestorsOf(tree, id)) collapsed.delete(up);
		await tick();
		const el = document.querySelector<HTMLElement>(`[data-node-${part}="${CSS.escape(id)}"]`);
		(el ?? document.getElementById('org-chart-heading'))?.focus();
	}

	/** Dialogs give focus back to the node they were opened from, wherever it is drawn now. */
	function restoreToNode(event: Event) {
		event.preventDefault();
		if (opened && opened.kind !== 'handover') void focusNode(opened.employeeId, 'menu');
	}

	function restoreToOwnerBar(event: Event) {
		event.preventDefault();
		void tick().then(() =>
			(
				document.querySelector<HTMLElement>('[data-handover]') ??
				document.getElementById('owner-bar-name')
			)?.focus()
		);
	}

	/** After a change made here: the route's data and the chart, read again. */
	async function reload() {
		await invalidate('app:org');
		await chart.reload();
	}

	// ---- the person just added: shown, focused and tinted for two seconds (§7.4) ----
	let justAdded = $state<string | undefined>();
	let tint: ReturnType<typeof setTimeout> | undefined;
	$effect(() => () => clearTimeout(tint));

	async function onadded(added: { memberId: string; name: string }) {
		await reload();
		justAdded = added.memberId;
		clearTimeout(tint);
		tint = setTimeout(() => (justAdded = undefined), 2000);
		// After the sheet has given focus back to its button.
		setTimeout(() => void focusNode(added.memberId, 'link'), 0);
	}

	function ontoggle(id: string) {
		if (collapsed.has(id)) collapsed.delete(id);
		else collapsed.add(id);
	}

	/** Arrow keys, Home and End between the nodes' profile links (TEAM-10). */
	function onTreeKey(event: KeyboardEvent) {
		const link = event.target as HTMLElement;
		const id = link.dataset?.nodeLink;
		if (!id || !tree || !TREE_KEYS.includes(event.key)) return;
		if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
		const move = treeMove(event.key, id, tree, collapsed);
		event.preventDefault();
		if (!move) return;
		if ('expand' in move) collapsed.delete(move.expand);
		else if ('collapse' in move) collapsed.add(move.collapse);
		else void focusNode(move.focus, 'link');
	}

	const explanation =
		"Reporting lines are explicit. Team leads also manage their team's subtree; ordinary ancestor-team members do not.";
</script>

<PageHeader
	title="Org chart"
	id="org-chart-heading"
	count={chart.data?.employees.length}
	countLabel={chart.data?.employees.length === 1 ? 'member' : 'members'}
	description={explanation}
>
	{#snippet actions()}
		<Button variant="outline" class={mobileButton} href={resolve(hireUrl() as '/')}
			>Hire an agent</Button
		>
		{#if snapshot && topTeamId && chart.state === 'ready'}
			<span data-add-person class="contents">
				<AddPersonToTeam {connection} {snapshot} teamId={topTeamId} {onadded} />
			</span>
		{/if}
	{/snippet}
</PageHeader>

{#if chart.state === 'loading'}
	<ChartSkeleton />
{:else}
	<Loaded
		resource={chart}
		label="the org chart"
		error="Couldn't load the org chart"
		isEmpty={(c) => c.employees.length === 0}
		emptyTitle="No one is on the org chart yet"
		emptyDescription="The org chart shows who reports to whom. Add a person to start it; agents join through Hire."
	>
		{#snippet children(data)}
			<div class="@container px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
				{#if owner}
					<OwnerBar
						{owner}
						organizationName={data.organization.name}
						isViewer={viewerOwns}
						onhandover={() => {
							opened = { kind: 'handover', n: ++openings };
							dialogOpen = true;
						}}
					/>
				{:else}
					<p class="text-sm/6 text-red-300">
						The owner's record isn't in this read, so the owner bar can't be shown. Everyone who is
						here is listed below.
					</p>
				{/if}

				{#if tree}
					<section aria-labelledby="no-manager-heading" class="mt-8">
						<h3 id="no-manager-heading" class="text-sm/6 font-semibold text-white">
							No explicit manager
							<span class="font-normal text-gray-400"
								><span aria-hidden="true">· {roots}</span><span class="sr-only"
									>, {roots} {roots === 1 ? 'member' : 'members'}</span
								></span
							>
						</h3>
						<p class="text-xs/5 text-gray-400">
							Everyone with no recorded manager. No manager is assumed for them.
						</p>
						<!-- The keys move between the nodes' links; each node's own controls stay in the Tab order. -->
						<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
						<ul role="list" class="mt-3" aria-label="Reporting lines" onkeydown={onTreeKey}>
							{#each tree.roots as node (node.employee.id)}
								<ChartNode
									{node}
									{collapsed}
									ownerId={data.ownerId}
									highlight={justAdded}
									canMove={!!snapshot}
									{ontoggle}
									{onaction}
								/>
							{/each}
						</ul>
					</section>
				{/if}
			</div>

			{#key opened?.n}
				{#if opened?.kind === 'manager' && target && tree}
					<ChangeManagerDialog
						bind:open={dialogOpen}
						{organizationId}
						employee={target}
						choices={managerChoices(data, tree, target.id)}
						onCloseAutoFocus={restoreToNode}
						onrefused={reload}
						ondone={async (r) => {
							await reload();
							toast.success(
								r.manager
									? `${r.name} now reports to ${r.manager}.`
									: `${r.name} has no explicit manager now.`
							);
							await focusNode(target.id, 'menu');
						}}
					/>
				{:else if opened?.kind === 'move' && target?.homeTeam && snapshot}
					<MoveHomeTeamDialog
						bind:open={dialogOpen}
						{organizationId}
						member={{ id: target.id, name: target.name }}
						fromTeamId={target.homeTeam.id}
						{snapshot}
						onCloseAutoFocus={restoreToNode}
						ondone={async (r) => {
							const id = target.id;
							await reload();
							if ('name' in r) toast.success(`Moved ${r.name} to ${r.to}.`);
							await focusNode(id, 'menu');
						}}
					/>
				{:else if opened?.kind === 'handover'}
					<HandOverDialog
						bind:open={dialogOpen}
						{organizationId}
						organizationName={data.organization.name}
						candidates={handOverCandidates(data)}
						onCloseAutoFocus={restoreToOwnerBar}
						ondone={async (r) => {
							await reload();
							toast.success(`${r.name} is now the owner of ${data.organization.name}.`);
							await tick();
							document.getElementById('owner-bar-name')?.focus();
						}}
					/>
				{/if}
			{/key}
		{/snippet}
	</Loaded>
{/if}
