<script lang="ts">
	import type { Snippet } from 'svelte';
	import { resolve } from '$app/paths';
	import type { SignalTone } from '$lib/modules/home/model';

	interface Props {
		id: string;
		title: string;
		titleHref: string;
		pillLabel: string;
		pillHref: string;
		pillTone: SignalTone;
		pillTitle?: string;
		heroHref: string;
		heroTitle?: string;
		subGridClass?: string;
		isFirst?: boolean;
		isLast?: boolean;
		icon: Snippet;
		hero: Snippet;
		sub: Snippet;
	}

	let {
		id,
		title,
		titleHref,
		pillLabel,
		pillHref,
		pillTone,
		pillTitle,
		heroHref,
		heroTitle,
		subGridClass = 'grid-cols-2 gap-3',
		isFirst = false,
		isLast = false,
		icon,
		hero,
		sub
	}: Props = $props();

	function pillToneClass(tone: SignalTone): string {
		if (tone === 'bad') return 'is-bad';
		if (tone === 'warn') return 'is-warn';
		if (tone === 'muted') return 'is-muted';
		return 'is-ok';
	}
</script>

<article
	class={[
		'kpi-pillar flex flex-col gap-3',
		isFirst && 'is-first',
		isLast && 'is-last',
		!isFirst && 'has-prev'
	]}
	aria-labelledby={id}
>
	<div class="flex items-center justify-between gap-2">
		<a
			{id}
			href={resolve(titleHref as '/projects')}
			class="pillar-name inline-flex items-center gap-1.5"
		>
			{@render icon()}
			<span>{title}</span>
		</a>
		<a
			href={resolve(pillHref as '/projects')}
			class={['status-pill', pillToneClass(pillTone)]}
			title={pillTitle}
		>
			{pillLabel}
		</a>
	</div>

	<a
		href={resolve(heroHref as '/projects')}
		class="pillar-link -m-1 flex flex-col gap-2 rounded-lg p-1"
		title={heroTitle}
	>
		{@render hero()}
	</a>

	<div class={['pillar-sub grid pt-2.5', subGridClass]}>
		{@render sub()}
	</div>
</article>

<style>
	.kpi-pillar {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-pillar-pad-sm);
		transition:
			background-color 0.16s ease,
			box-shadow 0.16s ease;
	}
	.kpi-pillar:hover {
		background: var(--exec-panel-bg-hover);
		box-shadow: var(--exec-panel-shadow-hover);
	}
	.kpi-pillar.has-prev {
		border-top: var(--exec-col-divider);
	}
	@media (min-width: 1280px) {
		.kpi-pillar {
			padding: var(--exec-pillar-pad);
		}
		.kpi-pillar.is-first {
			padding-left: var(--exec-pillar-edge-pad);
		}
		.kpi-pillar.is-last {
			padding-right: var(--exec-pillar-edge-pad);
		}
		.kpi-pillar.has-prev {
			border-top: 0;
			border-left: var(--exec-col-divider);
		}
	}
	.pillar-name {
		font-size: 0.7rem;
		line-height: 1rem;
		font-weight: 650;
		letter-spacing: 0.11em;
		text-transform: uppercase;
		color: var(--exec-pillar-name-ink);
		transition: color 0.14s ease;
	}
	.pillar-name:hover {
		color: var(--exec-ink);
	}
	.status-pill {
		padding: 2px 8px;
		border-radius: var(--exec-pill-radius);
		font-size: 0.7rem;
		font-weight: 600;
		white-space: nowrap;
	}
	.status-pill.is-ok {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: var(--exec-pill-ring-ok);
	}
	.status-pill.is-warn {
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: var(--exec-pill-ring-warn);
	}
	.status-pill.is-bad {
		background: var(--exec-bad-bg);
		color: var(--exec-bad);
		box-shadow: var(--exec-pill-ring-bad);
	}
	.status-pill.is-muted {
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: var(--exec-pill-ring-muted);
	}
	.pillar-link:hover {
		background: var(--exec-accent-soft);
	}
	.pillar-sub {
		border-top: 1px solid var(--exec-border);
	}
	@media (prefers-reduced-motion: reduce) {
		.kpi-pillar,
		.pillar-name {
			transition: none;
		}
	}
</style>
