<script lang="ts">
	import { resolve } from '$app/paths';
	import Check from '@lucide/svelte/icons/check';
	import Copy from '@lucide/svelte/icons/copy';
	import { splitHref, type TriageItem } from './triage-model';

	let {
		item,
		copiedKey,
		isAskReplyCutoff,
		snoozeReturnFull,
		onCopyWorkRef
	}: {
		item: TriageItem;
		copiedKey?: string;
		isAskReplyCutoff: boolean;
		snoozeReturnFull?: string;
		onCopyWorkRef: (itemId: string, ref: string) => void;
	} = $props();

	const workParts = $derived(item.workHref ? splitHref(item.workHref) : undefined);
	const relatedParts = $derived(item.relatedHref ? splitHref(item.relatedHref) : undefined);
</script>

<section class="triage-context" aria-labelledby="triage-context-title">
	<h3 id="triage-context-title">Request details</h3>
	<dl class="triage-fields">
		<div>
			<dt>Kind</dt>
			<dd>{item.kindDisplay}</dd>
		</div>

		<div>
			<dt>Project</dt>
			<dd>
				{#if item.projectId}
					<a
						class="triage-link"
						href={resolve(`/projects/${encodeURIComponent(item.projectId)}/work` as '/projects')}
					>
						{item.projectName}
					</a>
				{:else}
					<span>{item.projectName}</span>
				{/if}
			</dd>
		</div>

		{#if item.teamName}
			<div>
				<dt>Team</dt>
				<dd>{item.teamName}</dd>
			</div>
		{/if}

		{#if item.workRef}
			{@const isCopied = copiedKey === `${item.id}:${item.workRef}`}
			<div>
				<dt>Work item</dt>
				<dd>
					{#if workParts}
						<a
							class="triage-link"
							href="{resolve(workParts.pathname as '/work')}{workParts.suffix}"
						>
							{item.workRefDisplay ?? item.workRef}
						</a>
					{:else}
						<span>{item.workRefDisplay ?? item.workRef}</span>
					{/if}
					<button
						type="button"
						onclick={() => onCopyWorkRef(item.id, item.workRef!)}
						aria-label="Copy {item.workRef}"
						class={['triage-copy', isCopied && 'is-copied']}
					>
						{#if isCopied}
							<Check class="size-3.5 shrink-0" aria-hidden="true" />
							<span>Copied</span>
						{:else}
							<Copy class="size-3.5 shrink-0" aria-hidden="true" />
							<span>Copy</span>
						{/if}
					</button>
				</dd>
			</div>
		{/if}

		{#if item.expiresLabel}
			<div>
				<dt>{isAskReplyCutoff ? 'Reply cutoff' : 'Expires'}</dt>
				<dd class={item.state === 'open' ? 'triage-expiry' : undefined}>
					{item.expiresLabel}
				</dd>
			</div>
		{/if}

		{#if item.state === 'snoozed'}
			<div>
				<dt>Snoozed until</dt>
				<dd data-testid="detail-snoozed-until">
					{snoozeReturnFull ?? 'Scheduled'}
				</dd>
			</div>
			{#if item.snoozed?.note}
				<div>
					<dt>Snooze note</dt>
					<dd class="[overflow-wrap:anywhere]" data-testid="detail-snooze-note">
						{item.snoozed.note}
					</dd>
				</div>
			{/if}
		{/if}

		<div>
			<dt>Clears</dt>
			<dd>{item.clearsLabel}</dd>
		</div>

		{#if relatedParts}
			<div>
				<dt>Related page</dt>
				<dd>
					<a
						class="triage-link"
						href="{resolve(relatedParts.pathname as '/work')}{relatedParts.suffix}"
					>
						{item.openLabel ?? 'Open related page'}
					</a>
				</dd>
			</div>
		{/if}
	</dl>
</section>

<style>
	.triage-context {
		display: grid;
		min-width: 0;
		gap: 12px;
		padding-block: 12px 14px;
		border-block: 1px solid var(--exec-border);
	}
	.triage-context h3 {
		color: var(--exec-ink);
		font-size: 0.875rem;
		font-weight: 600;
	}
	.triage-fields {
		display: grid;
		gap: 10px 16px;
	}
	.triage-fields > div {
		display: grid;
		min-width: 0;
		align-content: start;
		gap: 2px;
	}
	@container inbox-reading (max-width: 29.99rem) {
		.triage-fields > div {
			grid-template-columns: minmax(5.25rem, 24%) minmax(0, 1fr);
			align-items: baseline;
			gap: 8px;
		}
	}
	@container inbox-reading (min-width: 30rem) {
		.triage-fields {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
	}
	@container inbox-reading (min-width: 47rem) {
		.triage-fields {
			grid-template-columns: repeat(3, minmax(0, 1fr));
		}
	}
	.triage-fields dt {
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-weight: 600;
	}
	.triage-fields dd {
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 4px 8px;
		min-width: 0;
		color: var(--exec-ink);
		font-size: 0.875rem;
		overflow-wrap: anywhere;
	}
	.triage-fields dd.triage-expiry {
		color: var(--tone-warn-ink);
	}
	.triage-link {
		display: inline-flex;
		align-items: center;
		min-width: 36px;
		min-height: 36px;
		color: var(--link);
		text-decoration: none;
	}
	.triage-link:is(:hover, :focus-visible) {
		text-decoration: underline;
	}
	.triage-copy {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		min-height: 36px;
		padding: 0 10px;
		border: 1px solid var(--exec-border-hi);
		border-radius: 6px;
		background: transparent;
		color: var(--exec-ink);
		font-size: 0.75rem;
		font-weight: 500;
		cursor: pointer;
	}
	.triage-copy:hover {
		background: var(--exec-btn-ghost-bg-hover);
	}
	.triage-copy.is-copied {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		border-color: var(--exec-ok-ring);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.triage-link,
		.triage-copy {
			min-width: 44px;
			min-height: 44px;
		}
	}
	.triage-link:focus-visible,
	.triage-copy:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (forced-colors: active) {
		.triage-copy {
			border: 1px solid ButtonText;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.triage-copy {
			transition: none;
		}
	}
</style>
