<script lang="ts">
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Info from '@lucide/svelte/icons/info';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import type { SpotlightDecision } from '$lib/modules/home/model';

	interface Props {
		tone: SpotlightDecision['recTone'];
		riskLabel?: string;
		recommendation: string;
	}

	let { tone, riskLabel, recommendation }: Props = $props();
</script>

<div
	class={['rec-callout flex items-start gap-2.5 px-3.5 py-2.5 text-sm', `tone-${tone}`]}
	data-tone={tone}
>
	{#if tone === 'ok'}
		<CheckCircle2 class="rec-ico mt-0.5 size-4 shrink-0" aria-hidden="true" />
	{:else if tone === 'warn'}
		<AlertTriangle class="rec-ico mt-0.5 size-4 shrink-0" aria-hidden="true" />
	{:else if tone === 'bad'}
		<ShieldAlert class="rec-ico mt-0.5 size-4 shrink-0" aria-hidden="true" />
	{:else}
		<Info class="rec-ico mt-0.5 size-4 shrink-0" aria-hidden="true" />
	{/if}
	<div class="min-w-0 flex-1">
		{#if riskLabel}
			<p class="rec-lbl mb-0.5 text-xs font-semibold tracking-wider uppercase">
				Recommendation · {riskLabel}
			</p>
		{/if}
		<p class="line-clamp-3 leading-relaxed">{recommendation}</p>
	</div>
</div>

<style>
	.rec-callout {
		--rec-ink: var(--exec-accent-ink);
		--rec-bar: var(--tone-accent);
		--rec-bg: var(--exec-accent-soft);
		--rec-ring: var(--exec-accent-ring);
		border-radius: var(--exec-inset-radius);
		background: var(--rec-bg);
		box-shadow:
			inset 0 0 0 1px var(--rec-ring),
			inset 3px 0 0 var(--rec-bar);
		color: var(--exec-body);
	}
	.rec-callout.tone-ok {
		--rec-ink: var(--exec-ok);
		--rec-bar: var(--tone-ok);
		--rec-bg: var(--exec-ok-bg);
		--rec-ring: var(--exec-ok-ring);
	}
	.rec-callout.tone-warn {
		--rec-ink: var(--exec-warn);
		--rec-bar: var(--tone-warn);
		--rec-bg: var(--exec-warn-bg);
		--rec-ring: var(--exec-warn-ring);
	}
	.rec-callout.tone-bad {
		--rec-ink: var(--exec-bad);
		--rec-bar: var(--tone-bad);
		--rec-bg: var(--exec-bad-bg);
		--rec-ring: var(--exec-bad-ring);
	}
	.rec-callout :global(.rec-ico),
	.rec-lbl {
		color: var(--rec-ink);
	}
</style>
