<!--
  @component
  Server-Validated Action Proposal / Confirmation Card for Embedded Tess (doc-97 §6 TESS-UX-8, TASK-80.5).
  Displays dry-run parameters, risk/reversibility badge (icon + word), consequence sentence,
  verb-object primary CTA (aria-busy during Apply), 'Edit in form' escape hatch, 'Cancel',
  inline role='alert' error recovery, and immutable server-confirmed receipt.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import type { TessActionProposal } from './types';

	let {
		proposal,
		onApply,
		onDismiss,
		onNavigate
	}: {
		proposal: TessActionProposal;
		onApply: (proposalId: string) => void;
		onDismiss: (proposalId: string) => void;
		onNavigate?: () => void;
	} = $props();

	const riskWord = $derived(
		proposal.riskLabel ??
			(proposal.risk === 'high'
				? 'High impact'
				: proposal.risk === 'review'
					? 'Review required'
					: 'Reversible')
	);
	const isBusy = $derived(proposal.status === 'applying');
</script>

<section
	class={['tess-prop-card', `is-${proposal.status}`]}
	data-testid="tess-proposal-card"
	data-proposal-id={proposal.id}
	aria-label={proposal.title}
>
	<div class="tess-prop-head">
		<strong class="tess-prop-title">{proposal.title}</strong>
		<span class={['tess-risk-pill', `tone-${proposal.risk}`]}>
			{#if proposal.risk === 'high'}
				<ShieldAlert class="size-3 shrink-0" aria-hidden="true" />
			{:else if proposal.risk === 'review'}
				<AlertTriangle class="size-3 shrink-0" aria-hidden="true" />
			{:else}
				<ShieldCheck class="size-3 shrink-0" aria-hidden="true" />
			{/if}
			<span>{riskWord}</span>
		</span>
	</div>

	{#if proposal.summary}
		<p class="tess-prop-summary">{proposal.summary}</p>
	{/if}

	{#if proposal.parameters.length > 0}
		<dl class="tess-prop-params">
			{#each proposal.parameters as param (param.label)}
				<div class="tess-prop-param">
					<dt>{param.label}</dt>
					<dd>{param.value}</dd>
				</div>
			{/each}
		</dl>
	{/if}

	{#if proposal.consequence && proposal.status !== 'applied' && proposal.status !== 'dismissed'}
		<p class="tess-prop-conseq">{proposal.consequence}</p>
	{/if}

	{#if proposal.status === 'applied'}
		<div class="tess-prop-receipt" role="status">
			<CheckCircle2 class="size-4 shrink-0 text-[var(--tone-ok-ink)]" aria-hidden="true" />
			<span class="min-w-0 flex-1">
				{proposal.receipt?.summary ?? 'Applied and recorded.'}
			</span>
			{#if proposal.receipt?.entityHref}
				<a
					href={resolve(proposal.receipt.entityHref as '/')}
					onclick={() => onNavigate?.()}
					class="tess-prop-link"
				>
					<span>{proposal.receipt.entityLabel ?? 'Open'}</span>
					<ExternalLink class="size-3" aria-hidden="true" />
				</a>
			{/if}
		</div>
	{:else if proposal.status === 'dismissed'}
		<p class="tess-prop-dismissed">Cancelled — no changes were made.</p>
	{:else}
		{#if proposal.errorMessage}
			<p class="tess-prop-err" role="alert">{proposal.errorMessage}</p>
		{/if}
		<div class="tess-prop-actions">
			<button
				type="button"
				disabled={isBusy}
				aria-busy={isBusy}
				onclick={() => onApply(proposal.id)}
				class="tess-prop-cta"
			>
				{#if isBusy}
					<LoaderCircle class="size-3.5 animate-spin" aria-hidden="true" />
				{/if}
				<span>{proposal.ctaLabel}</span>
			</button>
			{#if proposal.editHref}
				<a
					href={resolve(proposal.editHref as '/')}
					onclick={() => onNavigate?.()}
					class="tess-prop-sec"
				>
					Edit in form
				</a>
			{/if}
			<button
				type="button"
				disabled={isBusy}
				onclick={() => onDismiss(proposal.id)}
				class="tess-prop-sec"
			>
				Cancel
			</button>
		</div>
	{/if}
</section>

<style>
	.tess-prop-card {
		display: flex;
		flex-direction: column;
		gap: 8px;
		margin-top: 8px;
		padding: 12px;
		border-radius: var(--exec-inset-radius, 10px);
		background: var(--exec-surface);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.tess-prop-card.is-applied {
		box-shadow: inset 0 0 0 1px var(--tone-ok-ring);
	}
	.tess-prop-card.is-dismissed {
		opacity: 0.72;
	}
	.tess-prop-head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 6px;
	}
	.tess-prop-title {
		font-size: 0.875rem;
		font-weight: 600;
		color: var(--exec-ink);
	}
	.tess-risk-pill {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		padding: 2px 8px;
		border-radius: 999px;
		font-size: 0.68rem;
		font-weight: 600;
	}
	.tess-risk-pill.tone-reversible {
		background: var(--tone-ok-bg);
		color: var(--tone-ok-ink);
		box-shadow: inset 0 0 0 1px var(--tone-ok-ring);
	}
	.tess-risk-pill.tone-review {
		background: var(--tone-warn-bg);
		color: var(--tone-warn-ink);
		box-shadow: inset 0 0 0 1px var(--tone-warn-ring);
	}
	.tess-risk-pill.tone-high {
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
		box-shadow: inset 0 0 0 1px var(--tone-bad-ring);
	}
	.tess-prop-summary,
	.tess-prop-conseq {
		font-size: 0.78rem;
		line-height: 1.45;
		color: var(--exec-body);
	}
	.tess-prop-conseq {
		color: var(--exec-muted);
	}
	.tess-prop-params {
		display: grid;
		gap: 4px;
		padding: 8px 10px;
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.tess-prop-param {
		display: grid;
		grid-template-columns: minmax(76px, auto) minmax(0, 1fr);
		gap: 8px;
		font-size: 0.75rem;
	}
	.tess-prop-param dt {
		color: var(--exec-muted);
		font-weight: 500;
	}
	.tess-prop-param dd {
		color: var(--exec-ink);
		font-weight: 600;
		overflow-wrap: anywhere;
	}
	.tess-prop-err {
		padding: 7px 9px;
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
		box-shadow: inset 0 0 0 1px var(--tone-bad-ring);
		font-size: 0.75rem;
		line-height: 1.4;
	}
	.tess-prop-actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		padding-top: 2px;
	}
	.tess-prop-cta {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-height: 32px;
		padding: 0 12px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-cta-bg);
		color: var(--exec-on-accent);
		font-size: 0.78rem;
		font-weight: 600;
		cursor: pointer;
	}
	.tess-prop-cta:disabled {
		opacity: 0.65;
		cursor: wait;
	}
	.tess-prop-sec {
		display: inline-flex;
		align-items: center;
		min-height: 32px;
		padding: 0 10px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
		text-decoration: none;
	}
	.tess-prop-sec:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
	}
	.tess-prop-receipt {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		padding: 8px 10px;
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--tone-ok-bg);
		color: var(--exec-ink);
		font-size: 0.78rem;
		font-weight: 500;
	}
	.tess-prop-link {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--tone-ok-ink);
		font-weight: 600;
		text-decoration: none;
	}
	.tess-prop-link:hover {
		text-decoration: underline;
	}
	.tess-prop-dismissed {
		font-size: 0.75rem;
		color: var(--exec-muted);
	}
</style>
