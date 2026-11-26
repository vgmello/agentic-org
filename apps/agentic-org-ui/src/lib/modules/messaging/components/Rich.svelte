<script lang="ts">
	import { markdown, type Inline } from '../markdown';

	/** A message's text, drawn from the safe Markdown tree: text nodes only, links for http(s), chips for item numbers. */
	let { body }: { body: string } = $props();
	const blocks = $derived(markdown(body));
</script>

{#snippet inl(items: Inline[])}
	{#each items as i, k (k)}
		{#if i.t === 'text'}{i.text}{:else if i.t === 'bold'}<strong class="font-semibold text-fg"
				>{@render inl(i.children)}</strong
			>{:else if i.t === 'italic'}<em>{@render inl(i.children)}</em>{:else if i.t === 'code'}<code
				class="rounded bg-surface-overlay px-1 py-0.5 font-mono text-xs text-fg-bright"
				>{i.text}</code
			>{:else if i.t === 'link'}<a
				href={i.href}
				target="_blank"
				rel="noopener noreferrer"
				class="text-brand-text underline underline-offset-2 hover:text-brand-text-strong"
				title={i.host ? `Opens ${i.host}` : undefined}>{i.text}</a
			>{#if i.host}<span class="text-xs text-warning-text-strong" data-testid="link-host"
					>{' (opens ' + i.host + ')'}</span
				>{/if}{:else if i.t === 'chip'}<span
				class="rounded bg-surface-overlay px-1.5 py-0.5 font-mono text-xs font-semibold text-fg-bright"
				data-testid="item-chip">{i.ref}</span
			>{:else if i.t === 'mention'}<span
				class="rounded bg-primary/20 px-1 font-medium text-brand-text-strong"
				data-testid="mention">@{i.handle}</span
			>{/if}
	{/each}
{/snippet}

<div class="space-y-2 text-sm/6 text-fg-body [overflow-wrap:anywhere]">
	{#each blocks as b, k (k)}
		{#if b.t === 'p'}
			<p class="whitespace-pre-wrap">{@render inl(b.inline)}</p>
		{:else if b.t === 'quote'}
			<blockquote class="border-l-2 border-line-strong pl-3 whitespace-pre-wrap text-fg-secondary">
				{@render inl(b.inline)}
			</blockquote>
		{:else if b.t === 'ul'}
			<ul class="list-disc space-y-0.5 pl-5">
				{#each b.items as item, j (j)}<li>{@render inl(item)}</li>{/each}
			</ul>
		{:else}
			<pre
				class="overflow-x-auto rounded-md bg-shade/30 p-3 font-mono text-xs/5 text-fg-bright">{b.text}</pre>
		{/if}
	{/each}
</div>
