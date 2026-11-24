<!--
  @component
  Profile › Runs (agent-run-modes-ux §1, RUN-5; employee-supervision-ux §1): one row per logical turn, newest first:
  when · who started it · duration · cost (only when the viewer may see amounts; incomplete usage says so) · the
  outcome, its state as a word, the actual routed attempts with connection, model, outcome and usage, and Open
  session log when one was kept. Filter by outcome; the count of shown runs is announced.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Loaded from '$lib/components/Loaded.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { Button } from '$lib/components/ui/button';
	import { membersApi } from '../../api';
	import {
		ATTEMPT_OUTCOME,
		durationFact,
		OUTCOME_FILTERS,
		TURN_STATE,
		isOpen,
		matchesOutcome,
		outcomeText,
		startedByText,
		tokens,
		usd,
		whenText,
		type OutcomeFilter as Outcome
	} from '../../present';
	import type { Attempt, Turn, TurnState } from '../../types';
	import { profileContext } from '../profile/context.svelte';
	import { fact, factsList, factsWrap } from '../../components/facts';
	import OutcomeFilter from './OutcomeFilter.svelte';
	import { tabResource } from '../profile-tabs/reload.svelte';

	const ctx = profileContext();
	const runs = tabResource((o, m) => membersApi.runs(o, m));
	let filter = $state<Outcome>('all');
	const name = $derived(ctx.profile.name);

	const STATE_TONE: Record<TurnState, string> = {
		accepted: tone.info,
		running: 'bg-brand/10 text-brand-text ring-brand/30',
		switching: tone.info,
		blocked: 't-red',
		recovery_required: 't-red',
		succeeded: 't-green',
		failed: 't-red',
		cancelled: tone.neutral
	};

	const costText = (t: Turn) =>
		t.cost
			? `${usd(t.cost.usd)}${t.cost.inYourProjects ? ' in your projects' : ''}${t.cost.complete ? '' : ' (incomplete)'}`
			: undefined;

	const usageText = (a: Attempt) =>
		a.inputTokens === undefined && a.outputTokens === undefined
			? `usage ${a.usageComplete ? 'not reported' : 'incomplete'}`
			: `${tokens(a.inputTokens)} in · ${tokens(a.outputTokens)} out${a.usageComplete ? '' : ' (incomplete)'}`;

	const logHref = (t: Turn) =>
		`/members/${encodeURIComponent(ctx.profile.id)}/runs/${encodeURIComponent(t.id)}`;
	const runsWord = (n: number) => `${n} ${n === 1 ? 'run' : 'runs'}`;
</script>

<Loaded
	resource={runs}
	label="runs"
	error="Couldn't load {name}'s runs"
	isEmpty={(list) => list.length === 0}
	emptyTitle="No runs yet"
	emptyDescription="A run starts when {name} is given work, is messaged or mentioned, or someone uses Run now."
>
	{#snippet emptyAction()}
		{#if ctx.profile.status !== 'inactive'}
			<Button
				variant="outline"
				class={mobileButton}
				onclick={(e) => ctx.act('run', e.currentTarget)}>Run now</Button
			>
		{/if}
	{/snippet}
	{#snippet children(list)}
		{@const shown = list.filter((t) => matchesOutcome(t, filter))}
		{@const counts = Object.fromEntries(
			OUTCOME_FILTERS.map((f) => [f.id, list.filter((t) => matchesOutcome(t, f.id)).length])
		) as Record<Outcome, number>}
		<div class="px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
			<OutcomeFilter bind:value={filter} {counts} />
			<p class="sr-only" role="status">{runsWord(shown.length)} shown</p>
		</div>
		{#if shown.length === 0}
			<p
				class="border-t border-line-subtle px-4 py-8 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8"
			>
				No runs match this filter. <button
					type="button"
					class="rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					onclick={() => (filter = 'all')}>Show all runs</button
				>
			</p>
		{:else}
			<ul role="list" class="divide-y divide-line-subtle border-t border-line-subtle">
				{#each shown as t (t.id)}
					{@const cost = costText(t)}
					<li class="px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
						<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
							<p
								class="min-w-0 flex-1 basis-60 text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]"
							>
								{outcomeText(t)}
							</p>
							<StatusPill label={TURN_STATE[t.state]} class={STATE_TONE[t.state]} />
						</div>
						<div class="mt-1 {factsWrap}">
							<ul role="list" class="{factsList} text-xs/5 text-fg-muted tabular-nums">
								<li class={fact}><time datetime={t.startedAt}>{whenText(t.startedAt)}</time></li>
								<li class={fact}>{startedByText(t.startedBy)}</li>
								<li class={fact}>{durationFact(t)}</li>
								{#if cost}<li class={fact}>{cost}</li>{/if}
							</ul>
						</div>
						<div class="mt-3 flex flex-wrap items-start gap-x-6 gap-y-2">
							{#if t.attempts.length === 0}
								<p class="min-w-0 flex-1 basis-72 text-sm/6 text-fg-muted">
									No attempt yet: the office hasn't started this turn.
								</p>
							{:else}
								<details class="group min-w-0 flex-1 basis-72">
									<summary
										class="inline-flex min-h-6 cursor-pointer list-none items-center gap-x-1 rounded-sm text-sm/6 font-semibold text-fg-secondary hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 [&::-webkit-details-marker]:hidden"
									>
										<svg
											viewBox="0 0 20 20"
											fill="currentColor"
											aria-hidden="true"
											class="size-5 shrink-0 text-fg-muted transition-transform group-open:rotate-90 motion-reduce:transition-none"
											><path
												fill-rule="evenodd"
												d="M8.22 5.22a.75.75 0 0 1 1.06 0l4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.75.75 0 0 1-1.06-1.06L11.94 10 8.22 6.28a.75.75 0 0 1 0-1.06Z"
												clip-rule="evenodd"
											/></svg
										>
										{t.attempts.length === 1 ? '1 attempt' : `${t.attempts.length} attempts`}
									</summary>
									<ol role="list" class="mt-2 flex flex-col gap-2">
										{#each t.attempts as a (a.n)}
											<li
												class="rounded-md bg-surface-raised px-3 py-2 text-xs/5 text-fg-muted ring-1 ring-line ring-inset [overflow-wrap:anywhere]"
											>
												<p class="text-sm/6 text-fg-secondary">
													{a.n}. {a.connection} · <span class="font-mono">{a.model}</span>
												</p>
												<p>
													{ATTEMPT_OUTCOME[a.outcome]}{a.reason ? `: ${a.reason}` : ''} · {usageText(
														a
													)}
												</p>
											</li>
										{/each}
									</ol>
								</details>
							{/if}
							{#if t.hasLog}
								<a
									href={resolve(logHref(t) as '/')}
									class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
									>Open session log<span class="sr-only">, run of {whenText(t.startedAt)}</span></a
								>
							{:else if isOpen(t)}
								<p class="text-xs/5 text-fg-muted">Session log appears when the turn writes one</p>
							{:else}
								<p class="text-xs/5 text-fg-muted">No session log kept</p>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	{/snippet}
</Loaded>
