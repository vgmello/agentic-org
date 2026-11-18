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
	import { toAppError } from '$lib/office/api';
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
	import { formatName, previewLines, readSessionLog } from './read';

	let {
		organizationId,
		memberId,
		runId
	}: { organizationId: string; memberId: string; runId: string } = $props();

	// Not found (no log kept, or not this member's run) is a state of its own, without Retry.
	const log = new Resource(() =>
		readSessionLog(membersApi, organizationId, memberId, runId).catch((e) => {
			if (toAppError(e).code === 'not_found') return null;
			throw e;
		})
	);
	// Only the name, for the back link and the title; the log shows without it.
	const member = new Resource(() => membersApi.profile(organizationId, memberId));
	const name = $derived(member.data?.name);

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
		'rounded-sm font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400';
	const pre =
		'mt-2 overflow-x-auto rounded-md bg-black/20 p-3 font-mono text-xs/5 whitespace-pre-wrap text-gray-300 ring-1 ring-white/10 ring-inset [overflow-wrap:anywhere]';
	const toggle =
		'inline-flex min-h-9 cursor-pointer items-center gap-x-2 text-sm/6 text-gray-300 max-[639px]:min-h-11';
	const checkbox =
		'size-4 rounded-sm accent-indigo-500 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400';
</script>

<svelte:head><title>{pageTitle('Session log', name ?? 'Member', 'Teams')}</title></svelte:head>

<PageHeader
	title="Session log"
	back={{
		href: `/members/${encodeURIComponent(memberId)}/runs`,
		label: name ? `${name}'s runs` : 'Runs'
	}}
	description="Read-only. Taking over a session is a command on the machine of the office that runs it; there is no terminal in the browser."
/>

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
		<div class="border-b border-white/5 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
			<p class="text-xs/5 text-gray-400">
				{formatName(l)}{#if l.startedAt}&nbsp;· started <time datetime={l.startedAt}
						>{whenText(l.startedAt)}</time
					>{/if}{` · ${l.entries.length} entries`}
			</p>
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
				class="grid grid-cols-1 gap-px border-b border-white/5 bg-white/5 min-[640px]:grid-cols-2"
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
			<section aria-labelledby="{ids}-files" class="border-b border-white/5 py-4">
				<h3
					id="{ids}-files"
					class="px-4 text-sm/6 font-semibold text-white min-[640px]:px-6 min-[1024px]:px-8"
				>
					Files touched <span class="font-normal text-gray-400">{files.length}</span>
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
							<span class="text-xs/5 text-gray-400"
								>{ACTION[f.action]}{#if f.added || f.removed}&nbsp;· <span class="text-green-300"
										>+{f.added}</span
									>
									<span class="text-red-300">−{f.removed}</span><span class="sr-only">
										({f.added} lines added, {f.removed} removed)</span
									>{/if}</span
							>
						</li>
					{/each}
				</ul>
			</section>
		{/if}

		{#if shown.length === 0}
			<p class="px-4 py-8 text-sm/6 text-gray-400 min-[640px]:px-6 min-[1024px]:px-8">
				{l.entries.length === 0
					? 'This log has no entries.'
					: 'Every entry is hidden by the toggles above.'}
			</p>
		{:else}
			<ol role="list" class="divide-y divide-white/5" aria-label="Entries">
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
	<p class="flex flex-wrap gap-x-2 text-xs/5 font-semibold text-gray-400">
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
		<p class="mt-1 whitespace-pre-wrap text-gray-200 [overflow-wrap:anywhere]">{e.text}</p>
	{:else if e.kind === 'tool_call'}
		{@render meta(`Tool call · ${e.tool}`, e.at)}
		<p class="mt-1 font-mono text-xs/5 text-gray-300 [overflow-wrap:anywhere]">{e.summary}</p>
		{#if e.input}
			<details class="mt-1">
				<summary
					class="inline-flex cursor-pointer rounded-sm text-xs/5 text-gray-400 hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
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
		<p class="mt-1 font-mono text-xs/5 text-gray-200 [overflow-wrap:anywhere]">{e.path}</p>
		{#if e.diff?.length}
			<pre class={pre}>{#each e.diff as line, i (i)}<span
						class="block {line.op === 'add'
							? 'bg-green-500/10 text-green-300'
							: line.op === 'remove'
								? 'bg-red-500/10 text-red-300'
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
		<p class="mt-1 text-xs/5 text-gray-400">
			{tokens(e.inputTokens)} in · {tokens(e.outputTokens)} out{e.cachedTokens !== undefined
				? ` · ${tokens(e.cachedTokens)} cached`
				: ''}{e.complete ? '' : ' · incomplete'}
		</p>
	{:else if e.kind === 'notice'}
		{@render meta('Notice', e.at)}
		<p class="mt-1 text-gray-400 [overflow-wrap:anywhere]">{e.text}</p>
	{:else}
		{@render meta('Not recognised: shown as written', e.at)}
		<pre class={pre}>{e.source}</pre>
	{/if}
{/snippet}
