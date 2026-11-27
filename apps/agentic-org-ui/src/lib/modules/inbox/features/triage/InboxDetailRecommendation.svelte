<script lang="ts">
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Info from '@lucide/svelte/icons/info';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import type { TriageItem } from './triage-model';

	let {
		text,
		tone
	}: {
		text: string;
		tone: TriageItem['recommendationTone'];
	} = $props();

	const LABELS: Record<TriageItem['recommendationTone'], string> = {
		ok: 'Recommendation · Low risk',
		warn: 'Recommendation · Investigate',
		bad: 'Recommendation · Use caution',
		neutral: 'Recommendation'
	};
</script>

<div class="recommendation" data-tone={tone}>
	<span class="recommendation-ico" aria-hidden="true">
		{#if tone === 'ok'}
			<CheckCircle2 class="size-5" />
		{:else if tone === 'warn'}
			<AlertTriangle class="size-5" />
		{:else if tone === 'bad'}
			<ShieldAlert class="size-5" />
		{:else}
			<Info class="size-5" />
		{/if}
	</span>
	<div class="min-w-0">
		<h3 class="recommendation-label">{LABELS[tone]}</h3>
		<p>{text}</p>
	</div>
</div>

<style>
	.recommendation {
		--recommendation-ink: var(--exec-body);
		--recommendation-ring: var(--exec-border-hi);
		--recommendation-fill: var(--exec-well);
		display: flex;
		align-items: flex-start;
		gap: 10px;
		padding: 12px 14px;
		border: 1px solid var(--recommendation-ring);
		border-inline-start: 3px solid var(--recommendation-ink);
		border-radius: var(--exec-inset-radius);
		background: var(--recommendation-fill);
		color: var(--exec-ink);
		font-size: 0.875rem;
		line-height: 1.5;
		overflow-wrap: anywhere;
	}
	.recommendation p {
		max-width: 64ch;
	}
	.recommendation[data-tone='ok'] {
		--recommendation-ink: var(--tone-ok-ink);
		--recommendation-ring: var(--tone-ok-ring);
		--recommendation-fill: var(--tone-ok-bg);
	}
	.recommendation[data-tone='warn'] {
		--recommendation-ink: var(--tone-warn-ink);
		--recommendation-ring: var(--tone-warn-ring);
		--recommendation-fill: var(--tone-warn-bg);
	}
	.recommendation[data-tone='bad'] {
		--recommendation-ink: var(--tone-bad-ink);
		--recommendation-ring: var(--tone-bad-ring);
		--recommendation-fill: var(--tone-bad-bg);
	}
	.recommendation-ico,
	.recommendation-label {
		color: var(--recommendation-ink);
	}
	.recommendation-ico {
		display: inline-flex;
		flex: none;
		margin-top: 1px;
	}
	.recommendation-label {
		margin-bottom: 3px;
		font-size: 0.75rem;
		font-weight: 650;
		letter-spacing: 0.04em;
		text-transform: uppercase;
	}
	@media (forced-colors: active) {
		.recommendation {
			border: 1px solid ButtonText;
		}
	}
</style>
