<!--
  @component
  Chronological history list of BYOA external-agent session events (`TASK-249`, `doc-102` §4).
-->
<script lang="ts">
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { historyEventLabel, historyEventTone } from '../../external-auth-present';
	import type { ExternalHistoryEntry } from '../../external-auth-types';
	import { whenText } from '../../present';

	let { history }: { history: readonly ExternalHistoryEntry[] } = $props();
</script>

<div class="space-y-2.5" data-testid="external-auth-history-block">
	<h4 class="text-sm/6 font-semibold text-fg">Session history</h4>
	{#if history.length > 0}
		<ol role="list" class="divide-y divide-line-subtle rounded-md border border-line">
			{#each history as entry (entry.id)}
				<li
					class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1.5 px-4 py-3 text-xs/5"
				>
					<div class="flex min-w-0 flex-1 basis-52 flex-wrap items-center gap-2">
						<StatusPill
							label={historyEventLabel(entry.event)}
							class={historyEventTone(entry.event)}
						/>
						{#if entry.workspace_label || entry.workspace_path}
							<span class="font-medium text-fg-secondary [overflow-wrap:anywhere]">
								{entry.workspace_label ?? entry.workspace_path}
							</span>
						{/if}
						{#if entry.hostname}
							<span class="font-mono text-fg-muted [overflow-wrap:anywhere]">
								{entry.hostname}
							</span>
						{/if}
						{#if entry.actor_id}
							<span class="text-fg-muted [overflow-wrap:anywhere]">by {entry.actor_id}</span>
						{/if}
						{#if entry.pubkey_fingerprint}
							<span class="font-mono text-fg-muted [overflow-wrap:anywhere]">
								{entry.pubkey_fingerprint}
							</span>
						{/if}
						{#if entry.detail}
							<span class="text-fg-muted [overflow-wrap:anywhere]">{entry.detail}</span>
						{/if}
					</div>
					<time class="shrink-0 font-mono text-fg-muted" datetime={entry.at}>
						{whenText(entry.at)}
					</time>
				</li>
			{/each}
		</ol>
	{:else}
		<p class="text-sm/6 text-fg-muted">No prior external session history.</p>
	{/if}
</div>
