<script lang="ts">
	import Search from '@lucide/svelte/icons/search';
	import X from '@lucide/svelte/icons/x';
	import { mergeProps } from 'bits-ui';
	import * as Tooltip from '$lib/components/ui/tooltip';

	let {
		id,
		value = '',
		placeholder = 'Search…',
		label = 'Search',
		inputEl = $bindable<HTMLInputElement | null>(null),
		onValueChange,
		onClose
	}: {
		id?: string;
		value?: string;
		placeholder?: string;
		label?: string;
		inputEl?: HTMLInputElement | null;
		onValueChange: (next: string) => void;
		onClose?: () => void;
	} = $props();

	const uid = $props.id();
	const effectiveId = $derived(id ?? `${uid}-inline-search`);

	function handleKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229) return;
		if (event.key !== 'Escape') return;
		event.preventDefault();
		event.stopPropagation();
		if (value.trim().length > 0) {
			onValueChange('');
		} else if (onClose) {
			onClose();
		} else {
			inputEl?.blur();
		}
	}
</script>

<div class="inline-search-row">
	<Search class="inline-search-ico size-4 shrink-0" aria-hidden="true" />
	<label class="sr-only" for={effectiveId}>{label}</label>
	<input
		bind:this={inputEl}
		id={effectiveId}
		type="search"
		{value}
		oninput={(e) => onValueChange(e.currentTarget.value)}
		onkeydown={handleKeydown}
		{placeholder}
		autocomplete="off"
		class="inline-search-input"
	/>
	{#if value}
		<Tooltip.Provider delayDuration={150}>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...mergeProps(props, {
								onclick: () => {
									onValueChange('');
									inputEl?.focus();
								}
							})}
							type="button"
							aria-label="Clear search"
							class="inline-search-clear"
						>
							<X class="size-3.5" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content>Clear search</Tooltip.Content>
			</Tooltip.Root>
		</Tooltip.Provider>
	{:else if onClose}
		<Tooltip.Provider delayDuration={150}>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...mergeProps(props, { onclick: onClose })}
							type="button"
							aria-label="Close search"
							class="inline-search-clear"
						>
							<X class="size-3.5" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content>Close search</Tooltip.Content>
			</Tooltip.Root>
		</Tooltip.Provider>
	{/if}
</div>

<style>
	.inline-search-row {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px var(--triage-gutter, 24px);
		border-bottom: 1px solid var(--exec-border, var(--line-subtle));
		background: var(--exec-well, var(--surface-raised));
	}
	.inline-search-row :global(.inline-search-ico) {
		color: var(--exec-muted, var(--fg-muted));
	}
	.inline-search-input {
		flex: 1 1 auto;
		min-width: 0;
		min-height: 2rem;
		border: 0;
		background: transparent;
		color: var(--exec-ink, var(--fg));
		font-size: 0.875rem;
		outline: none;
	}
	.inline-search-input::placeholder {
		color: var(--exec-muted, var(--fg-muted));
		opacity: 1;
	}
	.inline-search-input::-webkit-search-cancel-button {
		-webkit-appearance: none;
		appearance: none;
	}
	.inline-search-clear {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 28px;
		min-height: 28px;
		border-radius: var(--exec-sm-radius, 6px);
		background: transparent;
		color: var(--exec-muted, var(--fg-muted));
		cursor: pointer;
	}
	.inline-search-clear:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
</style>
