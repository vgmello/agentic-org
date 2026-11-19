<!--
  @component
  One Activity row (our-org-in-the-app §6): time, actor, kind and text, a link to what it is about, Open thread
  for an authorized message, and for a recorded run a disclosure that expands in place to its trigger,
  duration, outcome and log summary. Feed rows expose actor, action and a `<time datetime>`; the full date,
  time and offset is in the accessible name of the time.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import { Button } from '$lib/components/ui/button';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { durationWords, eventTime, formatFull } from '$lib/modules/preferences/time';
	import { KIND_WORD, type ActivityRow } from '../../types';

	let {
		row,
		now,
		fresh = false,
		underDayHeader = true,
		onopenthread
	}: {
		row: ActivityRow;
		now: Date;
		/** Arrived a moment ago: highlighted for two seconds. */
		fresh?: boolean;
		underDayHeader?: boolean;
		onopenthread?: (row: ActivityRow, opener: HTMLElement) => void;
	} = $props();

	const id = $props.id();
	const when = $derived(eventTime(row.at, now, accountZone.zone, underDayHeader));
	const parts = $derived(row.text.includes('{link}') ? row.text.split('{link}') : [row.text, '']);
	let expanded = $state(false);

	const run = $derived(row.run);
	const runWords = $derived.by(() => {
		if (!run) return undefined;
		const started = new Date(run.started_at);
		if (run.state === 'running') return `running ${durationWords(started, now)}…`;
		const length = durationWords(started, new Date(run.ended_at ?? run.started_at));
		return run.state === 'failed' ? `failed after ${length}` : `ran ${length}`;
	});
</script>

<li
	data-testid="activity-row"
	data-row={row.id}
	data-kind={row.kind}
	tabindex="-1"
	class="relative flex gap-x-3 px-4 py-4 outline-none transition-colors duration-700 min-[640px]:px-6 min-[1024px]:px-8 motion-reduce:transition-none {fresh
		? 'bg-indigo-500/10 motion-safe:animate-in motion-safe:fade-in motion-safe:slide-in-from-top-2'
		: ''}"
>
	<Avatar name={row.actor.name} size="sm" class="mt-0.5" />
	<div class="min-w-0 flex-1 min-[640px]:flex min-[640px]:gap-x-4">
		<div class="min-w-0 flex-1">
			<p class="text-sm/6 text-gray-200 [overflow-wrap:anywhere]">
				<span class="font-semibold text-white">{row.actor.name}</span>
				<span class="sr-only">, {row.actor.kind === 'agent' ? 'Agent' : 'Person'},</span>
				{parts[0]}{#if row.href}<a
						href={resolve(row.href.path as '/')}
						class="rounded-sm font-medium text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
						>{row.href.label}</a
					>{parts[1]}{/if}
				{#if runWords}
					<span class="text-gray-400">·</span>
					<span
						class="whitespace-nowrap {run?.state === 'running'
							? 'text-indigo-300'
							: run?.state === 'failed'
								? 'text-red-300'
								: 'text-gray-300'}"
						data-testid="run-words">{runWords}</span
					>
				{/if}
			</p>
			{#if row.preview}
				<p class="mt-1 text-sm/6 text-gray-400 [overflow-wrap:anywhere]">“{row.preview}”</p>
			{/if}
			{#if run}
				<button
					type="button"
					aria-expanded={expanded}
					aria-controls="{id}-run"
					onclick={() => (expanded = !expanded)}
					class="mt-1 -ml-1 inline-flex min-h-6 items-center gap-x-1 rounded-sm px-1 text-xs/5 font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:min-h-11"
				>
					{expanded ? 'Hide run details' : 'Show run details'}
					<MiniIcon
						name="chevronDown"
						class="size-4 transition-transform motion-reduce:transition-none {expanded
							? 'rotate-180'
							: ''}"
					/>
				</button>
				{#if expanded}
					<dl
						id="{id}-run"
						class="mt-2 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 rounded-md border border-white/10 bg-white/[0.03] p-3 text-sm/6"
						data-testid="run-details"
					>
						<dt class="text-gray-400">Started by</dt>
						<dd class="text-gray-200 [overflow-wrap:anywhere]">{run!.trigger}</dd>
						<dt class="text-gray-400">Started</dt>
						<dd class="text-gray-200">{formatFull(new Date(run!.started_at), accountZone.zone)}</dd>
						<dt class="text-gray-400">Duration</dt>
						<dd class="text-gray-200">
							{durationWords(new Date(run!.started_at), new Date(run!.ended_at ?? now))}{run!
								.state === 'running'
								? ' so far'
								: ''}
						</dd>
						<dt class="text-gray-400">Outcome</dt>
						<dd class="text-gray-200 [overflow-wrap:anywhere]">
							{run!.state === 'running' ? 'Still running' : (run!.outcome ?? '')}
						</dd>
						{#if run!.log?.length}
							<dt class="text-gray-400">Log summary</dt>
							<dd class="min-w-0">
								<ul class="space-y-1 text-gray-200">
									{#each run!.log as line, index (index)}
										<li class="font-mono text-xs/5 [overflow-wrap:anywhere]">{line}</li>
									{/each}
								</ul>
							</dd>
						{/if}
					</dl>
					<a
						href={resolve(`/members/${run!.memberId}/runs/${run!.id}` as '/')}
						class="mt-2 inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:min-h-11"
						>Open session log</a
					>
				{/if}
			{/if}
		</div>
		<div
			class="mt-1 flex flex-wrap items-baseline gap-x-3 text-xs/5 text-gray-400 min-[640px]:order-first min-[640px]:mt-0.5 min-[640px]:block min-[640px]:w-28 min-[640px]:shrink-0"
		>
			<time datetime={when.iso} title={when.full} class="tabular-nums"
				>{when.text}<span class="sr-only"> ({when.full})</span></time
			>
			<span class="min-[640px]:block">{KIND_WORD[row.kind]}</span>
		</div>
	</div>
	{#if row.thread && onopenthread}
		<div class="shrink-0 self-start">
			<Button
				variant="outline"
				size="sm"
				class="max-[639px]:min-h-11"
				aria-label="Open thread with {row.thread.with}"
				onclick={(event) => onopenthread(row, event.currentTarget)}>Open thread</Button
			>
		</div>
	{/if}
</li>
