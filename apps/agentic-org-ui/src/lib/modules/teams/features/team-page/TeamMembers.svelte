<!--
  @component
  A team's Members tab (Prototype Release 1149b4f1, offices-and-teams-ux §7.2): the team's full roster, lead first.
  Each row: initials, name (opens the member's profile), Agent or Person in words, role, @handle,
  Lead when it leads the team, and its presence when known (unknown is omitted in rows, AC-IA08).
  Reflow (app-shell-ux §2): the text column is never narrower than 200 px. Where the avatar would squeeze it
  (320 px at 200% text) the text moves below the avatar; where the pill would, the pill wraps below the text.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import { PresenceBadge, kindWord, rowPresence } from '$lib/modules/members';
	import { membersOfMax } from '../../team-page';
	import type { TeamDetail } from '../../types';
	import { teamPageContext } from './context';
	import { fact, factsList, factsWrap } from './facts';

	let { detail: explicitDetail }: { detail?: TeamDetail } = $props();

	let ctx: ReturnType<typeof teamPageContext> | undefined;
	try {
		ctx = teamPageContext();
	} catch {
		ctx = undefined;
	}

	const detail = $derived(explicitDetail ?? ctx!.detail);
	const listId = $derived(`team-members-${detail.team.id}`);
</script>

<section
	aria-labelledby="{listId}-heading"
	class="px-4 py-5 min-[640px]:px-6 min-[1024px]:px-8"
	data-testid="team-members-roster"
>
	<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
		<h2 id="{listId}-heading" class="text-base/6 font-semibold text-fg">
			All members<span class="sr-only">:</span>
			<span class="ml-1.5 text-sm/6 font-normal text-fg-muted tabular-nums"
				>{membersOfMax(detail)}</span
			>
		</h2>
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
			{#each detail.members as m (m.id)}
				{@const presence = rowPresence({ presence: m.presence && { id: m.presence } })}
				<li
					class="relative -mx-2 flex flex-wrap items-center gap-x-3 gap-y-3 rounded-lg border-t border-line-subtle px-2 py-4 transition-colors duration-150 hover:bg-surface-raised/60 motion-reduce:transition-none"
				>
					<Avatar name={m.name} size="md" />
					<!-- Beside the avatar while that leaves it 200 px, else below it; the pill wraps below the text. -->
					<div
						data-member-text
						class="flex min-w-0 flex-[1_1_200px] flex-wrap items-center gap-x-3 gap-y-2"
					>
						<div class="min-w-0 flex-[1_1_12rem]">
							<p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm/6">
								<a
									href={resolve(`/members/${encodeURIComponent(m.id)}` as '/')}
									class="rounded-sm font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
									><span class="absolute inset-0" aria-hidden="true"></span>{m.name}<span
										class="sr-only">, {kindWord(m.kind).toLowerCase()}</span
									></a
								>
								{#if m.lead}<span
										class="inline-flex items-center rounded-full bg-brand/10 px-2 py-0.5 text-xs/4 font-medium text-brand-text ring-1 ring-brand/25 ring-inset"
										>Lead</span
									>{/if}
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
