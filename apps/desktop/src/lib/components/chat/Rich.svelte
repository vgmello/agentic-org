<script lang="ts">
	import { markdown, type Inline } from '$lib/chat/markdown';

	/** A message's text, drawn from the safe Markdown tree: text nodes only, links for http(s), chips for item numbers. */
	let { body }: { body: string } = $props();
	const blocks = $derived(markdown(body));
</script>

{#snippet inl(items: Inline[])}
	{#each items as i, k (k)}
		{#if i.t === 'text'}{i.text}{:else if i.t === 'bold'}<strong class="font-semibold text-white">{@render inl(i.children)}</strong>{:else if i.t === 'italic'}<em>{@render inl(i.children)}</em>{:else if i.t === 'code'}<code class="rounded bg-white/10 px-1 py-0.5 font-mono text-[0.85em] text-gray-100">{i.text}</code>{:else if i.t === 'link'}<a href={i.href} target="_blank" rel="noopener noreferrer" class="text-indigo-300 underline underline-offset-2 hover:text-indigo-200" title={i.host ? `Opens ${i.host}` : undefined}>{i.text}</a>{#if i.host}<span class="text-xs text-amber-200" data-testid="link-host">{' (opens ' + i.host + ')'}</span>{/if}{:else if i.t === 'chip'}<span class="rounded bg-white/10 px-1.5 py-0.5 font-mono text-xs font-semibold text-gray-100" data-testid="item-chip">{i.ref}</span>{:else if i.t === 'mention'}<span class="rounded bg-indigo-500/20 px-1 font-medium text-indigo-200" data-testid="mention">@{i.handle}</span>{/if}
	{/each}
{/snippet}

<div class="space-y-2 text-sm/6 text-gray-200 [overflow-wrap:anywhere]">
	{#each blocks as b, k (k)}
		{#if b.t === 'p'}
			<p class="whitespace-pre-wrap">{@render inl(b.inline)}</p>
		{:else if b.t === 'quote'}
			<blockquote class="border-l-2 border-white/20 pl-3 whitespace-pre-wrap text-gray-300">{@render inl(b.inline)}</blockquote>
		{:else if b.t === 'ul'}
			<ul class="list-disc space-y-0.5 pl-5">
				{#each b.items as item, j (j)}<li>{@render inl(item)}</li>{/each}
			</ul>
		{:else}
			<pre class="overflow-x-auto rounded-md bg-black/30 p-3 font-mono text-xs/5 text-gray-100">{b.text}</pre>
		{/if}
	{/each}
</div>
