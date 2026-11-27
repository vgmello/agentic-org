<script lang="ts">
	import Search from '@lucide/svelte/icons/search';

	let {
		open = false,
		hasQuery = false,
		label = 'Search',
		onToggle
	}: {
		open?: boolean;
		hasQuery?: boolean;
		label?: string;
		onToggle: () => void;
	} = $props();
</script>

<button
	type="button"
	onclick={onToggle}
	aria-label={label}
	aria-expanded={open}
	data-active={open || hasQuery ? '' : undefined}
	title="{label} (/)"
	class="inline-search-toggle"
>
	<Search class="size-4" aria-hidden="true" />
</button>

<style>
	.inline-search-toggle {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: var(--triage-control-height, 36px);
		min-height: var(--triage-control-height, 36px);
		border-radius: var(--exec-ctrl-radius, 8px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.inline-search-toggle:hover,
	.inline-search-toggle[aria-expanded='true'] {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.inline-search-toggle[data-active] {
		color: var(--tone-accent-ink, var(--brand-text));
	}
	.inline-search-toggle[data-active]::after {
		content: '';
		position: absolute;
		inset: 5px 5px auto auto;
		width: 5px;
		height: 5px;
		border-radius: 50%;
		background: currentColor;
	}
	.inline-search-toggle:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	@media (prefers-reduced-motion: reduce) {
		.inline-search-toggle {
			transition: none;
		}
	}
</style>
