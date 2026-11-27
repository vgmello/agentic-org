<!--
  @component
  Search & command results listbox shared by the Home inline palette and Cmd+K modal.
  Lazy-loaded on first interaction so the 18 Lucide icons stay out of shell first-paint JS.
-->
<script lang="ts">
	import Activity from '@lucide/svelte/icons/activity';
	import Building2 from '@lucide/svelte/icons/building-2';
	import ClipboardList from '@lucide/svelte/icons/clipboard-list';
	import DollarSign from '@lucide/svelte/icons/dollar-sign';
	import Folder from '@lucide/svelte/icons/folder';
	import Hash from '@lucide/svelte/icons/hash';
	import House from '@lucide/svelte/icons/house';
	import Inbox from '@lucide/svelte/icons/inbox';
	import Keyboard from '@lucide/svelte/icons/keyboard';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Palette from '@lucide/svelte/icons/palette';
	import PanelLeft from '@lucide/svelte/icons/panel-left';
	import Plus from '@lucide/svelte/icons/plus';
	import Search from '@lucide/svelte/icons/search';
	import Settings from '@lucide/svelte/icons/settings';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import UserPlus from '@lucide/svelte/icons/user-plus';
	import Users from '@lucide/svelte/icons/users';
	import type { CommandEntry, CommandIcon } from './command-palette-entries';

	let {
		entries,
		query,
		isChannelQuery,
		listboxId,
		optPrefix,
		preventBlur,
		safeActiveIdx,
		onSelectIndex,
		onRunEntry,
		onAskTess
	}: {
		entries: readonly CommandEntry[];
		query: string;
		isChannelQuery: boolean;
		listboxId: string;
		optPrefix: string;
		preventBlur: boolean;
		safeActiveIdx: number;
		onSelectIndex: (idx: number) => void;
		onRunEntry: (entry: CommandEntry) => void;
		onAskTess: (prompt: string) => void;
	} = $props();
</script>

{#snippet renderIcon(icon: CommandIcon)}
	{#if icon === 'plus'}
		<Plus class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'folder'}
		<Folder class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'sparkles'}
		<Sparkles class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'message'}
		<MessageSquare class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'user-plus'}
		<UserPlus class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'panel-left'}
		<PanelLeft class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'palette'}
		<Palette class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'keyboard'}
		<Keyboard class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'house'}
		<House class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'inbox'}
		<Inbox class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'clipboard'}
		<ClipboardList class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'activity'}
		<Activity class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'users'}
		<Users class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'building'}
		<Building2 class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'dollar'}
		<DollarSign class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'settings'}
		<Settings class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'hash'}
		<Hash class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else}
		<Search class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{/if}
{/snippet}

{#if entries.length === 0}
	<div class="flex flex-col items-center gap-2 py-7 text-center">
		<p class="text-sm text-[var(--exec-muted)]">
			{isChannelQuery ? `No channels matching “${query.trim()}”.` : `No matches for “${query}”.`}
		</p>
		{#if query.trim() && !isChannelQuery}
			<button
				type="button"
				onmousedown={preventBlur ? (e) => e.preventDefault() : undefined}
				onclick={() => onAskTess(query)}
				class="inline-flex min-h-9 items-center gap-1.5 rounded-lg bg-[var(--exec-accent-soft)] px-3 text-xs font-semibold text-[var(--exec-accent-ink)] shadow-[inset_0_0_0_1px_var(--exec-accent-ring)]"
			>
				<Sparkles class="size-3.5" aria-hidden="true" />
				<span>Ask Tess “{query.trim()}”</span>
			</button>
		{/if}
	</div>
{:else}
	<ul
		id={listboxId}
		class="exec-cmdk-list flex flex-col gap-[3px]"
		role="listbox"
		aria-label="Search results"
	>
		{#each entries as entry, idx (entry.id)}
			{@const showGroup = idx === 0 || entries[idx - 1].group !== entry.group}
			{#if showGroup}
				<li role="presentation" class={['exec-cmdk-grp', idx === 0 && 'is-first']}>
					{entry.group}
				</li>
			{/if}
			<li role="presentation">
				<button
					type="button"
					id="{optPrefix}-{idx}"
					role="option"
					tabindex={-1}
					aria-selected={idx === safeActiveIdx}
					onmousedown={preventBlur ? (e) => e.preventDefault() : undefined}
					onmouseenter={() => onSelectIndex(idx)}
					onclick={() => onRunEntry(entry)}
					class={[
						'exec-cmdk-opt flex w-full items-center text-left',
						idx === safeActiveIdx && 'is-active'
					]}
				>
					{@render renderIcon(entry.icon)}
					<span
						class="exec-cmdk-opt-txt flex min-w-0 flex-1 items-baseline gap-2 overflow-hidden"
					>
						<span class="exec-cmdk-opt-lbl shrink-0">{entry.label}</span>
						<span class="exec-cmdk-opt-sub hidden min-w-0 truncate sm:inline">
							{entry.sub}
						</span>
					</span>
					<span class="exec-cmdk-opt-tag shrink-0">
						{entry.tag}
					</span>
				</button>
			</li>
		{/each}
	</ul>
{/if}

<style>
	.exec-cmdk-grp {
		padding: 9px 12px 4px;
		font-size: 0.6875rem;
		line-height: 1.714;
		font-weight: 650;
		letter-spacing: var(--exec-cmdk-grp-tracking, 0.08em);
		text-transform: uppercase;
		color: var(--exec-dim);
	}
	.exec-cmdk-grp.is-first {
		padding-top: 4px;
	}
	.exec-cmdk-opt {
		gap: 11px;
		padding: 10px 12px;
		border-radius: var(--exec-cmdk-opt-radius, 10px);
		color: var(--exec-body);
		font-size: 0.875rem;
		line-height: 1.714;
		letter-spacing: 0.008em;
		cursor: pointer;
		transition:
			background-color 0.1s ease,
			color 0.1s ease;
	}
	.exec-cmdk-opt-lbl {
		font-weight: 500;
		color: inherit;
	}
	.exec-cmdk-opt-sub {
		font-size: 0.75rem;
		line-height: 1.714;
		color: var(--exec-dim);
	}
	.exec-cmdk-opt :global(.exec-cmdk-row-ico) {
		width: 17px;
		height: 17px;
		color: var(--exec-muted);
		transition: color 0.14s ease;
	}
	.exec-cmdk-opt-tag {
		padding: 1px 7px;
		border-radius: var(--exec-cmdk-tag-radius, 5px);
		background: var(--exec-cmdk-tag-bg, var(--exec-well));
		color: var(--exec-muted);
		font-size: 0.6875rem;
		line-height: 1.714;
		font-weight: 500;
	}
	.exec-cmdk-opt:hover {
		background: var(--exec-cmdk-opt-hover-bg, var(--exec-accent-soft));
		color: var(--exec-ink);
		box-shadow: var(
			--exec-cmdk-opt-hover-shadow,
			inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring))
		);
	}
	.exec-cmdk-opt.is-active {
		background: var(--exec-cmdk-opt-active-bg, var(--exec-accent-soft));
		color: var(--exec-ink);
		box-shadow: var(
			--exec-cmdk-opt-active-shadow,
			inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring))
		);
	}
	.exec-cmdk-opt:hover :global(.exec-cmdk-row-ico),
	.exec-cmdk-opt.is-active :global(.exec-cmdk-row-ico) {
		color: var(--exec-cmdk-opt-active-ico, var(--tone-accent-ink));
	}
	@media (max-width: 720px), (pointer: coarse) {
		.exec-cmdk-opt {
			height: auto;
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.exec-cmdk-opt,
		.exec-cmdk-opt :global(.exec-cmdk-row-ico) {
			transition: none;
		}
	}
</style>
