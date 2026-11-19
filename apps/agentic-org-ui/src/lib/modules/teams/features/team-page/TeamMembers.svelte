<!--
  @component
  The members above a team's tabs (§7.2): at most six rows, the lead first, then "All N members" to show the
  rest in place. Each row: initials, name (opens the member's profile), Agent or Person in words, role, @handle,
  Lead when it leads the team, and its presence when known (unknown is omitted in rows, AC-IA08).
  Reflow (app-shell-ux §2): the text column is never narrower than 200 px. Where the avatar would squeeze it
  (320 px at 200% text) the text moves below the avatar; where the pill would, the pill wraps below the text.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import { Button } from '$lib/components/ui/button';
	import { PresenceBadge, kindWord, rowPresence } from '$lib/modules/members';
	import { MEMBERS_SHOWN, membersOfMax, shownMembers } from '../../team-page';
	import type { TeamDetail } from '../../types';
	import { fact, factsList, factsWrap } from './facts';

	let { detail }: { detail: TeamDetail } = $props();

	let expanded = $state(false);
	const rows = $derived(shownMembers(detail.members, expanded));
	const listId = $derived(`team-members-${detail.team.id}`);
</script>

<section
	aria-labelledby="{listId}-heading"
	class="border-b border-line-subtle px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8"
>
	<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
		<h3 id="{listId}-heading" class="text-sm/6 font-semibold text-fg">
			Members<span class="sr-only">:</span>
			<span class="ml-1 font-normal text-fg-muted">{membersOfMax(detail)}</span>
		</h3>
		{#if detail.members.length > MEMBERS_SHOWN}
			<Button
				variant="ghost"
				class="-my-1.5 text-link hover:text-brand-text max-[639px]:min-h-11"
				aria-expanded={expanded}
				aria-controls={listId}
				onclick={() => (expanded = !expanded)}
				>{expanded ? 'Fewer members' : `All ${detail.members.length} members`}</Button
			>
		{/if}
	</div>
	{#if detail.members.length === 0}
		<p class="mt-2 text-sm/6 text-fg-muted">
			No one has {detail.team.name} as their home team yet.{detail.team.status === 'archived'
				? ''
				: ' Add a person, or move someone here with Move home team on their profile.'}
		</p>
	{:else}
		<ul
			id={listId}
			role="list"
			class="mt-3 grid grid-cols-1 gap-x-8 min-[1024px]:grid-cols-2 min-[1440px]:grid-cols-3"
		>
			{#each rows as m (m.id)}
				{@const presence = rowPresence({ presence: m.presence && { id: m.presence } })}
				<li
					class="relative flex flex-wrap items-center gap-x-3 gap-y-3 border-t border-line-subtle py-4"
				>
					<Avatar name={m.name} size="md" />
					<!-- Beside the avatar while that leaves it 200 px, else below it; the pill wraps below the text. -->
					<div
						data-member-text
						class="flex min-w-0 flex-[1_1_200px] flex-wrap items-center gap-x-3 gap-y-2"
					>
						<div class="min-w-0 flex-[1_1_12rem]">
							<p class="flex flex-wrap items-baseline gap-x-2 text-sm/6">
								<a
									href={resolve(`/members/${encodeURIComponent(m.id)}` as '/')}
									class="rounded-sm font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
									><span class="absolute inset-0" aria-hidden="true"></span>{m.name}<span
										class="sr-only">, {kindWord(m.kind).toLowerCase()}</span
									></a
								>
								{#if m.lead}<span class="text-xs/5 font-medium text-brand-text">Lead</span>{/if}
							</p>
							<div class={factsWrap}>
								<ul role="list" class="{factsList} text-xs/5 text-fg-muted">
									<li class={fact} aria-hidden="true">{kindWord(m.kind)}</li>
									{#if m.role}<li class={fact}>{m.role}</li>{/if}
									<li class={fact}>@{m.handle}</li>
								</ul>
							</div>
						</div>
						{#if presence}<PresenceBadge {presence} class="relative" />{/if}
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</section>
