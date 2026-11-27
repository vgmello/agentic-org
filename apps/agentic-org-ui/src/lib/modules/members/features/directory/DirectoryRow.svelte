<!--
  @component
  One divided row of the Members directory (offices-and-teams-ux §7.0, §7.2, PEOPLE-1): initials, the name (the row's
  one link, to the member's profile, spoken "Nadia, agent"), the Owner badge, Agent or Person in words, role and
  @handle, Can sign in for a person with an account, the agent's office and freshness or Unplaced (a person's contact
  instead), the one home team, "Working on" when known, and one presence pill when known (unknown is omitted,
  AC-IA08).

  The row is a grid. Stacked (the default): the avatar in its own track and every fact below the name in the text
  track, the presence pill under the name's lines. Under 20rem of list (320 px at 200% text) the avatar sits above
  the text, so the words keep the full width. When the directory's list is at least 56rem wide (a container
  query in rem, so 200% text falls back to stacked rows) the row becomes a subgrid of the list: avatar · member ·
  home team · working on · presence, lined up with the header.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import PresenceBadge from '../../components/PresenceBadge.svelte';
	import { kindWord, rowPresence } from '../../present';
	import type { MemberRow } from '../../types';
	import { formerText, placementText } from './directory';

	let {
		member,
		highlight = false
	}: {
		member: MemberRow;
		/** Just added: tinted for two seconds (§7.4). */
		highlight?: boolean;
	} = $props();

	// A former member has no presence, task, office, account or ownership to show: deactivation removes access and
	// the owner is never deactivated (§7.1, §7.4), whatever a stale record says.
	const former = $derived(member.status === 'inactive');
	const presence = $derived(former ? undefined : rowPresence(member));
	const placement = $derived(former ? undefined : placementText(member));
	const working = $derived(former ? undefined : member.workingOn);
	const owner = $derived(member.owner && !former);
	const canSignIn = $derived(member.canSignIn && !former);
</script>

<li
	class="relative grid grid-cols-[2.25rem_minmax(0,1fr)] gap-x-4 gap-y-1 px-4 py-4 transition-colors duration-150 hover:bg-surface-raised/60 motion-reduce:transition-none min-[640px]:px-6 @max-[20rem]:grid-cols-1 @min-[56rem]:col-span-full @min-[56rem]:grid-cols-subgrid @min-[56rem]:items-center {highlight
		? 'bg-primary/10'
		: ''}"
	data-member={member.id}
	data-kind={member.kind}
>
	<Avatar name={member.name} size="md" class="col-start-1 row-start-1" />

	<div class="col-start-2 row-start-1 min-w-0 @max-[20rem]:col-start-1 @max-[20rem]:row-start-2">
		<p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm/6">
			<a
				href={resolve(`/members/${encodeURIComponent(member.id)}` as '/')}
				data-member-link={member.id}
				aria-label="{member.name}, {kindWord(member.kind).toLowerCase()}"
				class="rounded-sm font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
				><span class="absolute inset-0" aria-hidden="true"></span>{member.name}</a
			>
			<span
				aria-hidden="true"
				class={[
					'inline-flex items-center rounded-full px-2 py-0.5 text-xs/4 font-semibold',
					member.kind === 'agent'
						? 'member-kind-agent'
						: 'bg-surface-overlay text-fg-secondary ring-1 ring-line ring-inset'
				]}>{kindWord(member.kind)}</span
			>
			<!-- The member profile's Owner badge (ProfileFrame), so the owner reads the same everywhere. -->
			{#if owner}<StatusPill label="Owner" class="member-owner-pill" />{/if}
		</p>
		<p class="mt-1 flex flex-wrap gap-x-2 text-xs/5 text-fg-muted">
			{#if member.role}<span class="[overflow-wrap:anywhere]">{member.role}</span><span
					aria-hidden="true">·</span
				>{/if}
			<span class="font-mono [overflow-wrap:anywhere]">@{member.handle}</span>
			{#if canSignIn}<span aria-hidden="true">·</span><span class="text-fg-secondary"
					>Can sign in</span
				>{/if}
		</p>
		{#if placement}
			<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">{placement}</p>
		{/if}
	</div>

	{#if presence}
		<!-- StatusPill truncates its label; here the words wrap whole instead (app-shell-ux §6a), inside the row. -->
		<div
			class="relative col-start-2 max-w-full min-w-0 justify-self-start @max-[20rem]:col-start-1 @min-[56rem]:col-start-5 @min-[56rem]:row-start-1 [&_[title]]:overflow-visible [&_[title]]:whitespace-normal"
		>
			<PresenceBadge {presence} class="max-w-full" />
		</div>
	{/if}

	<p
		class="col-start-2 text-xs/5 text-fg-muted [overflow-wrap:anywhere] @max-[20rem]:col-start-1 @min-[56rem]:col-start-3 @min-[56rem]:row-start-1 @min-[56rem]:text-sm/6"
	>
		{#if former}
			{formerText(member)}
		{:else if member.homeTeam}
			<span class="@min-[56rem]:sr-only">Home team</span>
			<span class="@min-[56rem]:text-fg-secondary">{member.homeTeam.name}</span>
		{:else}
			<span class="text-danger-text">No home team on record: this record is invalid</span>
		{/if}
	</p>

	{#if working}
		<p
			class="col-start-2 text-xs/5 text-fg-muted [overflow-wrap:anywhere] @max-[20rem]:col-start-1 @min-[56rem]:col-start-4 @min-[56rem]:row-start-1 @min-[56rem]:text-sm/6"
		>
			<span class="@min-[56rem]:sr-only">Working on</span>
			<span
				class="rounded bg-surface-raised px-1.5 py-0.5 font-mono text-xs/4 text-fg-muted tabular-nums ring-1 ring-line ring-inset"
				>{working.ref}</span
			>
			<span class="ml-1 text-fg-secondary">{working.title}</span>
		</p>
	{/if}
</li>

<style>
	.member-kind-agent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	li :global(.member-owner-pill) {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
</style>
