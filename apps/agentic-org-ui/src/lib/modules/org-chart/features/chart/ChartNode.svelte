<!--
  @component
  One employee in the reporting tree (offices-and-teams-ux §7.8, REPORT-2, TEAM-12, PEOPLE-6) and, below it, the
  people who report to them. The node shows initials, the name (its profile link, spoken "Noor Haddad, agent"),
  Agent or Person in words with the @handle and the one home team; an agent's office and freshness, or Unplaced;
  "Leads <team>" beside the number of explicit reports, so both sources of responsibility show (REPORT-5); Owner and
  Paused in words. A manager has a collapse/expand control naming them; every node has an actions menu.
  Nested lists keep the hierarchy for screen readers; indentation stops growing past level two on a narrow tree so
  the text keeps its width (320 px, 200% text).
-->
<script lang="ts" module>
	export type NodeAction = 'manager' | 'move';
</script>

<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import { kindWord, reportedText } from '$lib/modules/members';
	import { listText, reportsText, type TreeNode } from '../../tree';
	import type { ChartEmployee } from '../../types';
	import Self from './ChartNode.svelte';

	let {
		node,
		collapsed,
		ownerId,
		highlight,
		canMove,
		ontoggle,
		onaction
	}: {
		node: TreeNode;
		collapsed: ReadonlySet<string>;
		ownerId: string;
		/** Just added: tinted for two seconds (§7.4). */
		highlight: string | undefined;
		/** Move home team needs the team tree; without it the item is left out. */
		canMove: boolean;
		ontoggle: (id: string) => void;
		onaction: (action: NodeAction, employee: ChartEmployee) => void;
	} = $props();

	const e = $derived(node.employee);
	const hasReports = $derived(node.children.length > 0);
	const open = $derived(hasReports && !collapsed.has(e.id));
	const reports = $derived(reportsText(node.children.length));
	const placement = $derived(
		e.kind !== 'agent'
			? undefined
			: e.office
				? `${e.office.name} · ${reportedText(e.office.observedAt)}`
				: 'Unplaced'
	);
	const pill =
		'inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-xs/5 font-medium whitespace-nowrap ring-1 ring-inset';
	const meta = 'text-xs/5 text-fg-muted [overflow-wrap:anywhere]';
</script>

<li data-node={e.id} data-kind={e.kind}>
	<!--
	  Toggle · avatar · name · menu, the facts under the name (app-shell-ux §2: no text column under 200 px). Under 24rem
	  of tree (a phone) it stacks: toggle · name · menu on the first line and the facts across the whole row below,
	  without the avatar or a leaf's empty toggle space. Under 16rem (320 px at 200% text) the controls take the first
	  line, close above the name they belong to, and name and facts take the full width. Columns are spaced by
	  margins, so a hidden column leaves no gap. The row tints on hover and keyboard focus, tying the menu to it.
	-->
	<div
		class="grid grid-cols-[auto_auto_minmax(0,1fr)_auto] items-start rounded-md px-1 py-3 transition-colors duration-150 has-focus-visible:bg-surface-raised hover:bg-surface-raised motion-reduce:transition-none @max-[16rem]:pt-1 @max-[16rem]:pb-4 {highlight ===
		e.id
			? 'bg-primary/10'
			: ''}"
	>
		{#if hasReports}
			<button
				type="button"
				class="col-start-1 row-start-1 mr-3 -my-0.5 flex size-7 shrink-0 items-center justify-center rounded-md text-fg-muted hover:bg-surface-raised hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:-my-2.5 max-[639px]:size-11"
				aria-expanded={open}
				aria-controls="reports-{e.id}"
				aria-label="{open ? 'Collapse' : 'Expand'} {e.name}'s reports"
				data-node-toggle={e.id}
				onclick={() => ontoggle(e.id)}
			>
				<svg
					viewBox="0 0 20 20"
					fill="currentColor"
					aria-hidden="true"
					class="size-5 transition-transform duration-150 motion-reduce:transition-none {open
						? 'rotate-90'
						: ''}"
					><path
						fill-rule="evenodd"
						d="M8.22 5.22a.75.75 0 0 1 1.06 0l4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.75.75 0 0 1-1.06-1.06L11.94 10 8.22 6.28a.75.75 0 0 1 0-1.06Z"
						clip-rule="evenodd"
					/></svg
				>
			</button>
		{:else}
			<span
				aria-hidden="true"
				class="col-start-1 row-start-1 mr-3 size-7 max-[639px]:size-11 @max-[24rem]:hidden"
			></span>
		{/if}
		<Avatar
			name={e.name}
			size="md"
			class="col-start-2 row-span-2 row-start-1 mr-3 -mt-1.5 @max-[24rem]:hidden"
		/>
		<div
			class="col-start-3 row-start-1 min-w-0 @max-[16rem]:col-span-full @max-[16rem]:col-start-1 @max-[16rem]:row-start-2"
		>
			<p class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-sm/6">
				<a
					href={resolve(`/members/${encodeURIComponent(e.id)}` as '/')}
					data-node-link={e.id}
					aria-label="{e.name}, {kindWord(e.kind).toLowerCase()}"
					class="inline-flex min-h-6 items-center rounded-sm font-semibold text-fg hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 [overflow-wrap:anywhere]"
					>{e.name}</a
				>
				{#if e.id === ownerId}<span class="{pill} bg-brand/10 text-brand-text ring-brand/30"
						>Owner</span
					>{/if}
				{#if e.status === 'paused'}<span class="{pill} t-amber">Paused</span>{/if}
			</p>
		</div>
		<DropdownMenu>
			<DropdownMenuTrigger>
				{#snippet child({ props })}
					<Button
						{...props}
						variant="ghost"
						size="icon"
						data-node-menu={e.id}
						class="col-start-4 row-start-1 ml-3 -my-0.5 size-7 shrink-0 text-fg-muted hover:text-fg max-[639px]:-my-2.5 max-[639px]:size-11"
						aria-label="Actions for {e.name}"
					>
						<svg viewBox="0 0 20 20" fill="currentColor" class="size-5" aria-hidden="true"
							><path
								d="M3 10a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0ZM8.5 10a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0ZM15.5 8.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3Z"
							/></svg
						>
					</Button>
				{/snippet}
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end">
				<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => onaction('manager', e)}
					>Change manager…</DropdownMenuItem
				>
				{#if canMove && e.homeTeam}
					<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => onaction('move', e)}
						>Move home team…</DropdownMenuItem
					>
				{/if}
			</DropdownMenuContent>
		</DropdownMenu>
		<div
			data-facts
			class="col-start-3 row-start-2 min-w-0 @max-[24rem]:col-span-full @max-[24rem]:col-start-1 @max-[16rem]:row-start-3"
		>
			<p class={meta}>
				{kindWord(e.kind)} · @{e.handle} ·
				{#if e.homeTeam}{e.homeTeam.name}{:else}<span class="text-danger-text"
						>No home team on record</span
					>{/if}
			</p>
			{#if placement}<p class={meta} data-placement>{placement}</p>{/if}
			{#if e.leads.length || reports}
				<p class={meta}>
					{[e.leads.length ? `Leads ${listText(e.leads.map((t) => t.name))}` : '', reports ?? '']
						.filter(Boolean)
						.join(' · ')}
				</p>
			{/if}
			{#if node.problem}<p class="text-xs/5 text-danger-text [overflow-wrap:anywhere]">
					{node.problem}
				</p>{/if}
		</div>
	</div>
	{#if hasReports}
		<ul
			id="reports-{e.id}"
			role="list"
			aria-label="{e.name}'s reports"
			hidden={!open}
			class="ml-[0.875rem] border-l border-line pl-2 max-[639px]:ml-[1.375rem] @max-[16rem]:ml-1 @max-[16rem]:pl-[8px] {node.level >=
			2
				? '@max-[30rem]:ml-0 @max-[30rem]:border-l-0 @max-[30rem]:pl-0'
				: ''}"
		>
			{#each node.children as child (child.employee.id)}
				<Self node={child} {collapsed} {ownerId} {highlight} {canMove} {ontoggle} {onaction} />
			{/each}
		</ul>
	{/if}
</li>
