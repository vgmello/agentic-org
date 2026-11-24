<!--
  @component
  The read-only session log of one run (REQ-05, PROF-2): the conversation as one list of entries (messages, tool calls
  and results, files read or changed with their diffs, usage and notices), whatever runtime wrote it. Two toggles hide
  or show tool calls and the file views (the touched-files list and each change). A line the parser doesn't recognise
  is shown as it was. There is no terminal here: taking over a session is a command on the office's machine.
-->
<script lang="ts">
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import StatTile from '$lib/components/StatTile.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { pageTitle } from '$lib/nav';
	import { resolve } from '$app/paths';
	import { toAppError } from '$lib/proto';
	import { membersApi } from '../../api';
	import { tokens, whenText } from '../../present';
	import {
		PREVIEW_LINES,
		touchedFiles,
		usageTotals,
		visibleEntries,
		type FileAction,
		type SessionEntry
	} from './entries';
	import { fact, factsList, factsWrap } from '../../components/facts';
	import { formatName, previewLines, readSessionLog } from './read';
	import { runFacts } from './run-header';

	let {
		organizationId,
		memberId,
		memberName,
		runId
	}: {
		organizationId: string;
		memberId: string;
		/** Known before the page draws (or not at all), so the back link's label never changes. */
		memberName?: string;
		runId: string;
	} = $props();

	// Not found (no log kept, or not this member's run) is a state of its own, without Retry.
	const log = new Resource(() =>
		readSessionLog(membersApi, organizationId, memberId, runId).catch((e) => {
			if (toAppError(e).code === 'not_found') return null;
			throw e;
		})
	);
	// The run this log belongs to, for the header line; the log shows without it.
	const run = new Resource(async () =>
		(await membersApi.runs(organizationId, memberId)).find((t) => t.id === runId)
	);
	const name = $derived(memberName);

	let showTools = $state(true);
	let showFiles = $state(true);
	let expanded = $state<Record<string, boolean>>({});
	const ids = $props.id();

	const ACTION: Record<FileAction, string> = {
		read: 'Read',
		edit: 'Changed',
		create: 'Created',
		delete: 'Deleted',
		reference: 'Mentioned'
	};
	const ROLE = $derived({ user: 'Prompt', assistant: name ?? 'Agent', system: 'System' } as const);
	const linkClass =
		'rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
	const pre =
		'mt-2 overflow-x-auto rounded-md bg-surface-sunken p-3 font-mono text-xs/5 whitespace-pre-wrap text-fg-secondary ring-1 ring-line ring-inset [overflow-wrap:anywhere]';
	const toggle =
		'inline-flex min-h-9 cursor-pointer items-center gap-x-2 text-sm/6 text-fg-secondary max-[639px]:min-h-11';
	const checkbox =
		'size-4 rounded-sm accent-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
</script>

<svelte:head><title>{pageTitle('Session log', name ?? 'Member', 'Teams')}</title></svelte:head>

<PageHeader
	title="Session log"
	back={{
		href: `/members/${encodeURIComponent(memberId)}/runs`,
		label: name ? `${name}'s runs` : 'Runs'
	}}
	description={header}
/>

{#snippet header()}
	<!-- The run's line keeps its height while it loads, so the page doesn't shift when it arrives. -->
	<div class="min-h-6">
		{#if run.data}
			<div class={factsWrap}>
				<ul role="list" class="{factsList} text-sm/6 text-fg-secondary" aria-label="This run">
					{#each runFacts(run.data) as text, i (i)}<li class={fact}>{text}</li>{/each}
				</ul>
			</div>
		{:else if run.state === 'loading'}
			<span
				class="mt-1 block h-4 w-64 max-w-full animate-pulse rounded bg-surface-overlay motion-reduce:animate-none"
				aria-hidden="true"
			></span>
		{/if}
	</div>
	<p class="mt-1">
		Read-only. Taking over a session is a command on the machine of the office that runs it; there
		is no terminal in the browser.
	</p>
{/snippet}

<Loaded
	resource={log}
	label="session log"
	error="Couldn't load the session log"
	rows={6}
	isEmpty={(value) => value === null}
	emptyLevel={2}
	emptyTitle="No session log"
	emptyDescription="No session log was kept for this run, or it isn't one of {name ??
		'this member'}'s runs."
>
	{#snippet emptyAction()}
		<a href={resolve(`/members/${encodeURIComponent(memberId)}/runs` as '/')} class={linkClass}
			>{name ? `Open ${name}'s runs` : 'Open the runs'}</a
		>
	{/snippet}
	{#snippet children(found)}
		{@const l = found!}
		{@const shown = visibleEntries(l.entries, { tools: showTools, files: showFiles })}
		{@const files = touchedFiles(l.entries)}
		{@const usage = usageTotals(l.entries)}
		<div class="border-b border-line-subtle px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
			<div class={factsWrap}>
				<ul role="list" class="{factsList} text-xs/5 text-fg-muted">
					<li class={fact}>{formatName(l)}</li>
					{#if l.startedAt}<li class={fact}>
							started <time datetime={l.startedAt}>{whenText(l.startedAt)}</time>
						</li>{/if}
					<li class={fact}>{l.entries.length} {l.entries.length === 1 ? 'entry' : 'entries'}</li>
				</ul>
			</div>
			<fieldset class="mt-3 flex flex-wrap gap-x-6 gap-y-1">
				<legend class="sr-only">Show in the log</legend>
				<label class={toggle}
					><input type="checkbox" bind:checked={showTools} class={checkbox} />Show tool calls</label
				>
				<label class={toggle}
					><input type="checkbox" bind:checked={showFiles} class={checkbox} />Show files</label
				>
			</fieldset>
			<p class="sr-only" role="status">{shown.length} of {l.entries.length} entries shown</p>
		</div>

		{#if usage}
			<dl
				class="grid grid-cols-1 gap-px border-b border-line-subtle bg-surface-raised min-[640px]:grid-cols-2"
			>
				<StatTile label="Input tokens" value={tokens(usage.input)} />
				<StatTile
					label="Output tokens"
					value={tokens(usage.output)}
					detail={usage.complete ? undefined : 'Usage incomplete: some was not reported'}
				/>
			</dl>
		{/if}

		{#if showFiles && files.length}
			<section aria-labelledby="{ids}-files" class="border-b border-line-subtle py-4">
				<h3
					id="{ids}-files"
					class="px-4 text-sm/6 font-semibold text-fg min-[640px]:px-6 min-[1024px]:px-8"
				>
					Files touched <span class="font-normal text-fg-muted">{files.length}</span>
				</h3>
				<ul role="list" class="mt-2">
					{#each files as f (f.path)}
						<li
							class="flex flex-wrap items-baseline gap-x-3 px-4 py-1 text-sm/6 min-[640px]:px-6 min-[1024px]:px-8"
						>
							<a
								href="#entry-{f.entryIds[0]}"
								class="font-mono font-normal [overflow-wrap:anywhere] {linkClass}">{f.path}</a
							>
							<span class="text-xs/5 text-fg-muted"
								>{ACTION[f.action]}{#if f.added || f.removed}&nbsp;· <span class="text-success-text"
										>+{f.added}</span
									>
									<span class="text-danger-text">−{f.removed}</span><span class="sr-only">
										({f.added} lines added, {f.removed} removed)</span
									>{/if}</span
							>
						</li>
					{/each}
				</ul>
			</section>
		{/if}

		{#if shown.length === 0}
			<p class="px-4 py-8 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
				{l.entries.length === 0
					? 'This log has no entries.'
					: 'Every entry is hidden by the toggles above.'}
			</p>
		{:else}
			<ol role="list" class="divide-y divide-line-subtle" aria-label="Entries">
				{#each shown as e (e.id)}
					<li
						id="entry-{e.id}"
						class="scroll-mt-20 px-4 py-4 text-sm/6 min-[640px]:px-6 min-[1024px]:px-8"
					>
						{@render entry(e)}
					</li>
				{/each}
			</ol>
		{/if}
	{/snippet}
</Loaded>

{#snippet meta(label: string, at: string | undefined)}
	<p class="flex flex-wrap gap-x-2 text-xs/5 font-semibold text-fg-muted">
		<span>{label}</span>
		{#if at}<span aria-hidden="true">·</span><time datetime={at} class="font-normal"
				>{whenText(at)}</time
			>{/if}
	</p>
{/snippet}

{#snippet long(id: string, text: string)}
	{@const cut = previewLines(text, PREVIEW_LINES)}
	<pre class={pre}>{expanded[id] ? text : cut.shown}</pre>
	{#if cut.hidden}
		<button
			type="button"
			class="mt-1 text-xs/5 {linkClass} max-[639px]:min-h-11"
			aria-expanded={!!expanded[id]}
			onclick={() => (expanded[id] = !expanded[id])}
			>{expanded[id] ? 'Show less' : `Show all (${cut.hidden} more lines)`}</button
		>
	{/if}
{/snippet}

{#snippet entry(e: SessionEntry)}
	{#if e.kind === 'message'}
		{@render meta(ROLE[e.role], e.at)}
		<p class="mt-1 whitespace-pre-wrap text-fg-body [overflow-wrap:anywhere]">{e.text}</p>
	{:else if e.kind === 'tool_call'}
		{@render meta(`Tool call · ${e.tool}`, e.at)}
		<p class="mt-1 font-mono text-xs/5 text-fg-secondary [overflow-wrap:anywhere]">{e.summary}</p>
		{#if e.input}
			<details class="mt-1">
				<summary
					class="inline-flex cursor-pointer rounded-sm text-xs/5 text-fg-muted hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					>Input</summary
				>
				<pre class={pre}>{e.input}</pre>
			</details>
		{/if}
	{:else if e.kind === 'tool_result'}
		{@render meta(e.ok ? 'Tool result' : 'Tool result · failed', e.at)}
		{@render long(e.id, e.output)}
	{:else if e.kind === 'file'}
		{@render meta(`File · ${ACTION[e.action]}`, e.at)}
		<p class="mt-1 font-mono text-xs/5 text-fg-body [overflow-wrap:anywhere]">{e.path}</p>
		{#if e.diff?.length}
			<pre class={pre}>{#each e.diff as line, i (i)}<span
						class="block {line.op === 'add'
							? 'bg-success-emphasis/10 text-success-text'
							: line.op === 'remove'
								? 'bg-danger-emphasis/10 text-danger-text'
								: ''}"
						><span class="sr-only"
							>{line.op === 'add' ? 'Added: ' : line.op === 'remove' ? 'Removed: ' : ''}</span
						><span aria-hidden="true"
							>{line.op === 'add' ? '+ ' : line.op === 'remove' ? '− ' : '  '}</span
						>{line.text}</span
					>{/each}</pre>
		{/if}
	{:else if e.kind === 'usage'}
		{@render meta('Usage', e.at)}
		<p class="mt-1 text-xs/5 text-fg-muted">
			{tokens(e.inputTokens)} in · {tokens(e.outputTokens)} out{e.cachedTokens !== undefined
				? ` · ${tokens(e.cachedTokens)} cached`
				: ''}{e.complete ? '' : ' · incomplete'}
		</p>
	{:else if e.kind === 'notice'}
		{@render meta('Notice', e.at)}
		<p class="mt-1 text-fg-muted [overflow-wrap:anywhere]">{e.text}</p>
	{:else}
		{@render meta('Not recognised: shown as written', e.at)}
		<pre class={pre}>{e.source}</pre>
	{/if}
{/snippet}
