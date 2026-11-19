<!--
  @component
  Hire, Describe (our-org-in-the-app §7): "What do you need?" and the required Home team, prefilled from the launching
  team. The picker lists only the teams the viewer may hire into (§7.1), with their member count; a full team is
  refused with its limit named. When the HR lead can't run (§2a) the description is disabled with the reason and the
  primary action is Fill in the details yourself.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import {
		errorClass,
		helpClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { hrUnavailableText, isFull, teamFullText, teamsInTreeOrder } from '../../hire';
	import type { HireTeam, HrLeadStatus } from '../../types';

	let {
		teams,
		hr,
		reason = $bindable(),
		homeTeamId = $bindable(),
		busy = false,
		error,
		onnext,
		onmanual
	}: {
		teams: HireTeam[];
		hr: HrLeadStatus;
		reason: string;
		homeTeamId: string;
		busy?: boolean;
		/** Why asking the HR lead failed. */
		error?: string;
		/** Ask the HR lead to draft. */
		onnext: () => void;
		/** Fill in the details yourself. */
		onmanual: () => void;
	} = $props();

	const cantRun = $derived(hrUnavailableText(hr));
	const team = $derived(teams.find((t) => t.id === homeTeamId));
	const full = $derived(team && isFull(team) ? teamFullText(team) : undefined);
	const blocked = $derived(
		full
			? 'Choose a team with room to continue'
			: !team
				? cantRun
					? 'Choose a home team to continue'
					: reason.trim()
						? 'Choose a home team to continue'
						: 'Describe what you need and choose a home team to continue'
				: !cantRun && !reason.trim()
					? 'Describe what you need to continue'
					: undefined
	);
</script>

<div class="max-w-2xl space-y-6">
	<div>
		<Label for="hire-need" class="text-sm/6 font-medium text-fg">What do you need?</Label>
		<textarea
			id="hire-need"
			rows="4"
			class="mt-2 {textareaClass} disabled:cursor-not-allowed disabled:opacity-60"
			placeholder={cantRun ? '' : 'Someone to test checkout end to end before each release'}
			disabled={!!cantRun}
			aria-describedby="hire-need-help"
			bind:value={reason}></textarea>
		<p id="hire-need-help" class={helpClass}>
			{cantRun
				? `${cantRun} Hires are filled in by hand until it can run; nothing waits for it.`
				: 'The HR lead reads this and drafts a proposal: role, title, name, focus and how it runs. You check it before anything is created.'}
		</p>
	</div>

	<div>
		<Label for="hire-team" class="text-sm/6 font-medium text-fg">Home team</Label>
		<select
			id="hire-team"
			class="mt-2 {selectClass}"
			aria-invalid={full ? true : undefined}
			aria-describedby="hire-team-help"
			bind:value={homeTeamId}
		>
			{#if !team}<option value="" disabled>Choose a team</option>{/if}
			{#each teamsInTreeOrder(teams) as { team: t, depth } (t.id)}
				<option value={t.id}
					>{'\u00a0\u00a0'.repeat(depth)}{t.name} · {t.members} of {t.maxMembers} members</option
				>
			{/each}
		</select>
		{#if full && team}
			<p id="hire-team-help" class={errorClass}>
				{full}
				<a
					href={resolve(`/teams/${encodeURIComponent(team.id)}` as '/')}
					class="rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					>Open {team.name}</a
				>
			</p>
		{:else}
			<p id="hire-team-help" class={helpClass}>
				Where the new agent belongs. Only teams you may hire into are listed. Projects and work are
				assigned later.
			</p>
		{/if}
	</div>

	{#if error}
		<p role="alert" class="text-sm/6 text-danger-text [overflow-wrap:anywhere]">{error}</p>
	{/if}

	<div class="flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-line-subtle pt-6">
		<Button
			class="min-w-28 {mobileButton}"
			disabled={!!blocked}
			aria-busy={busy}
			aria-describedby={blocked ? 'hire-describe-blocked' : undefined}
			onclick={() => (busy ? undefined : cantRun ? onmanual() : onnext())}
			>{cantRun ? 'Fill in the details yourself' : busy ? 'Asking the HR lead…' : 'Next'}</Button
		>
		{#if blocked}
			<p id="hire-describe-blocked" class="text-xs/5 text-fg-muted">{blocked}</p>
		{/if}
	</div>
</div>
