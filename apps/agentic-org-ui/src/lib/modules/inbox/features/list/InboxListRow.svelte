<!--
  @component
  A compact Inbox row: kind, what, who and where, and the time that matters (when it lapses, else how long
  ago it came). One stretched link opens the item. Used in the Inbox list and on Home's Needs you.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { eventTime, formatFull, relativeUntil } from '$lib/modules/preferences/time';
	import { whoLine } from '../../items';
	import { KIND_WORD, type InboxItem } from '../../types';

	let {
		item,
		href,
		now,
		current = false,
		class: className = ''
	}: {
		item: InboxItem;
		href: string;
		now: Date;
		/** The item shown in the detail pane. */
		current?: boolean;
		class?: string;
	} = $props();

	const expiry = $derived(item.expires_at ? new Date(item.expires_at) : undefined);
	const arrived = $derived(eventTime(item.created_at, now, accountZone.zone));
</script>

<li
	class="relative flex gap-x-3 px-4 py-4 min-[640px]:px-6 {current
		? 'bg-white/5'
		: 'hover:bg-white/[0.03]'} {className}"
	data-testid="inbox-row"
	data-item={item.id}
>
	<Avatar name={item.who.name} size="md" />
	<div class="min-w-0 flex-1">
		<p class="text-xs/5 text-gray-400">
			<span class="font-semibold text-gray-300">{KIND_WORD[item.kind]}</span>
			{#if item.state === 'handled' && item.handled}<span> · {item.handled.outcome}</span>{/if}
		</p>
		<p class="mt-0.5 text-sm/6 font-semibold text-white [overflow-wrap:anywhere]">
			<a
				href={resolve(href as '/')}
				aria-current={current ? 'true' : undefined}
				class="focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400"
				><span class="absolute inset-0" aria-hidden="true"></span>{item.title}</a
			>
		</p>
		<p class="mt-0.5 text-xs/5 text-gray-400 [overflow-wrap:anywhere]">{whoLine(item)}</p>
		{#if item.state === 'open'}
			<p class="mt-1 text-xs/5 text-gray-400">
				{#if expiry}
					<time datetime={expiry.toISOString()} title={formatFull(expiry, accountZone.zone)}
						>Expires {relativeUntil(expiry, now)}<span class="sr-only">
							({formatFull(expiry, accountZone.zone)})</span
						></time
					>
				{:else}
					<time datetime={arrived.iso} title={arrived.full}
						>{arrived.text}<span class="sr-only"> ({arrived.full})</span></time
					>
				{/if}
			</p>
		{/if}
	</div>
</li>
