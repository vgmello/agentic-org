<!--
  @component
  Conversations someone takes part in: the channel, direct conversation or thread, its last line and when.
  Each row opens Chat.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { whenText } from '../present';
	import type { ConversationRow } from '../types';

	let { rows }: { rows: ConversationRow[] } = $props();
	const KIND = { channel: 'Channel', direct: 'Direct conversation', thread: 'Thread' } as const;
</script>

<ul role="list" class="divide-y divide-line-subtle">
	{#each rows as row (row.id)}
		<li class="relative px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
			<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
				<p class="min-w-0 text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">
					<a
						href={resolve(`/chat?c=${encodeURIComponent(row.id)}` as '/')}
						class="rounded-sm focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
						><span class="absolute inset-0" aria-hidden="true"></span>{row.title}</a
					>
				</p>
				<p class="text-xs/5 text-fg-muted">
					{KIND[row.kind]} · <time datetime={row.lastAt}>{whenText(row.lastAt)}</time>
				</p>
			</div>
			<p class="mt-1 truncate text-sm/6 text-fg-muted" title={row.preview}>{row.preview}</p>
		</li>
	{/each}
</ul>
