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
		OUTCOME_FILTERS,
		TURN_STATE,
		matchesOutcome,
		outcomeText,
		startedByText,
		tokens,
		turnDuration,
		usd,
		whenText,
		type OutcomeFilter
	} from '../../present';
	import type { Attempt, Turn, TurnState } from '../../types';
	import { profileContext } from '../profile/context.svelte';
	import { tabResource } from '../profile-tabs/reload.svelte';

	const ctx = profileContext();
	const runs = tabResource((o, m) => membersApi.runs(o, m));
	let filter = $state<OutcomeFilter>('all');
	const name = $derived(ctx.profile.name);

	const STATE_TONE: Record<TurnState, string> = {
		accepted: tone.info,
		running: 'bg-indigo-400/10 text-indigo-300 ring-indigo-400/30',
		switching: tone.info,
		blocked: tone.bad,
		recovery_required: tone.bad,
		succeeded: tone.ok,
		failed: tone.bad,
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
		<fieldset
			class="flex flex-wrap items-center gap-2 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<legend class="sr-only">Show runs</legend>
			{#each OUTCOME_FILTERS as f (f.id)}
				<label
					class="inline-flex min-h-9 cursor-pointer items-center rounded-md px-3 text-sm/6 font-semibold ring-1 ring-inset has-checked:bg-white/10 has-checked:text-white has-checked:ring-white/20 has-checked:underline has-checked:underline-offset-4 has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-indigo-400 max-[639px]:min-h-11 {filter ===
					f.id
						? ''
						: 'text-gray-400 ring-white/10 hover:text-white'}"
				>
					<input type="radio" name="run-filter" value={f.id} bind:group={filter} class="sr-only" />
					{f.label}
				</label>
			{/each}
			<p class="sr-only" role="status">{runsWord(shown.length)} shown</p>
		</fieldset>
		{#if shown.length === 0}
			<p
				class="border-t border-white/5 px-4 py-8 text-sm/6 text-gray-400 min-[640px]:px-6 min-[1024px]:px-8"
			>
				No runs match this filter. <button
					type="button"
					class="rounded-sm font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
					onclick={() => (filter = 'all')}>Show all runs</button
				>
			</p>
		{:else}
			<ul role="list" class="divide-y divide-white/5 border-t border-white/5">
				{#each shown as t (t.id)}
					{@const cost = costText(t)}
					<li class="px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
						<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
							<p
								class="min-w-0 flex-1 basis-60 text-sm/6 font-semibold text-white [overflow-wrap:anywhere]"
							>
								{outcomeText(t)}
							</p>
							<StatusPill label={TURN_STATE[t.state]} class={STATE_TONE[t.state]} />
						</div>
						<p class="mt-1 flex flex-wrap gap-x-2 text-xs/5 text-gray-400">
							<time datetime={t.startedAt}>{whenText(t.startedAt)}</time>
							<span aria-hidden="true">·</span><span>{startedByText(t.startedBy)}</span>
							<span aria-hidden="true">·</span><span
								>{t.endedAt ? `ran ${turnDuration(t)}` : turnDuration(t)}</span
							>
							{#if cost}<span aria-hidden="true">·</span><span>{cost}</span>{/if}
						</p>
						<div class="mt-3 flex flex-wrap items-start gap-x-6 gap-y-2">
							<details class="group min-w-0 flex-1 basis-72">
								<summary
									class="inline-flex min-h-6 cursor-pointer list-none items-center [&::-webkit-details-marker]:hidden gap-x-1 rounded-sm text-sm/6 font-semibold text-gray-300 hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:min-h-11"
								>
									<svg
										viewBox="0 0 20 20"
										fill="currentColor"
										aria-hidden="true"
										class="size-5 shrink-0 text-gray-400 transition-transform group-open:rotate-90 motion-reduce:transition-none"
										><path
											fill-rule="evenodd"
											d="M8.22 5.22a.75.75 0 0 1 1.06 0l4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.75.75 0 0 1-1.06-1.06L11.94 10 8.22 6.28a.75.75 0 0 1 0-1.06Z"
											clip-rule="evenodd"
										/></svg
									>
									{t.attempts.length === 1 ? '1 attempt' : `${t.attempts.length} attempts`}
								</summary>
								<ol role="list" class="mt-2 space-y-2">
									{#each t.attempts as a (a.n)}
										<li
											class="rounded-md bg-white/[0.03] px-3 py-2 text-xs/5 text-gray-400 ring-1 ring-white/10 ring-inset [overflow-wrap:anywhere]"
										>
											<p class="text-sm/6 text-gray-300">
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
							{#if t.hasLog}
								<a
									href={resolve(logHref(t) as '/')}
									class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:min-h-11"
									>Open session log<span class="sr-only">, run of {whenText(t.startedAt)}</span></a
								>
							{:else}
								<p class="text-xs/5 text-gray-400">No session log kept</p>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	{/snippet}
</Loaded>
