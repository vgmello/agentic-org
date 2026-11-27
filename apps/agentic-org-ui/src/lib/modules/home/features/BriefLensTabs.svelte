<script lang="ts">
	import { tick } from 'svelte';
	import type { BriefLens } from '$lib/modules/home/model';

	const BRIEF_LENSES: readonly { id: BriefLens; label: string }[] = [
		{ id: 'today', label: 'Today' },
		{ id: 'week', label: '7-day' },
		{ id: 'board', label: 'Board' }
	];

	interface Props {
		lens: BriefLens;
	}

	let { lens = $bindable<BriefLens>('today') }: Props = $props();

	function handleKeydown(event: KeyboardEvent, index: number) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step && event.key !== 'Home' && event.key !== 'End') return;
		event.preventDefault();
		const nextIdx =
			event.key === 'Home'
				? 0
				: event.key === 'End'
					? BRIEF_LENSES.length - 1
					: (index + step + BRIEF_LENSES.length) % BRIEF_LENSES.length;
		const nextLens = BRIEF_LENSES[nextIdx].id;
		lens = nextLens;
		void tick().then(() => document.getElementById(`exec-brief-tab-${nextLens}`)?.focus());
	}
</script>

<div
	class="brief-lenses inline-flex items-center gap-0.5 p-0.5"
	role="tablist"
	aria-label="Brief timeframe"
>
	{#each BRIEF_LENSES as option, idx (option.id)}
		<button
			type="button"
			role="tab"
			id="exec-brief-tab-{option.id}"
			aria-selected={lens === option.id}
			aria-controls="exec-brief-scroll"
			tabindex={lens === option.id ? 0 : -1}
			onclick={() => (lens = option.id)}
			onkeydown={(e) => handleKeydown(e, idx)}
			class={[
				'lens-tab inline-flex min-h-8 items-center px-2.5 py-1 text-xs font-semibold',
				lens === option.id && 'is-on'
			]}
		>
			{option.label}
		</button>
	{/each}
</div>

<style>
	.brief-lenses {
		border-radius: var(--exec-lenses-radius);
		background: var(--exec-well);
		box-shadow: var(--exec-lenses-shadow);
	}
	.lens-tab {
		border-radius: var(--exec-sm-radius);
		color: var(--exec-muted);
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.lens-tab:hover {
		color: var(--exec-ink);
	}
	.lens-tab.is-on {
		background: var(--exec-lens-on-bg);
		color: var(--exec-lens-on-ink);
		box-shadow: var(--exec-lens-on-shadow);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.lens-tab {
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.lens-tab {
			transition: none;
		}
	}
</style>
