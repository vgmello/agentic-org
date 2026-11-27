<script lang="ts">
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import type { ExecutiveBriefSection } from '$lib/modules/home/model';

	interface Props {
		sections: readonly ExecutiveBriefSection[];
	}

	let { sections }: Props = $props();

	function splitFirstSentence(text: string): { first: string; rest: string } {
		const trimmed = text.trim();
		const match = trimmed.match(/^(.+?[.!?])(?:\s+([\s\S]+))?$/);
		if (!match) return { first: trimmed, rest: '' };
		return { first: match[1].trim(), rest: (match[2] ?? '').trim() };
	}

	function firstSentence(text: string): string {
		return splitFirstSentence(text).first;
	}

	const highlightLayout = $derived.by(() => {
		const leadSec = sections[0];
		if (!leadSec) {
			return {
				leadSec: undefined,
				leadText: '',
				showLeadKicker: false,
				rows: [] as { id: string; kicker: string; tone: 'ok' | 'warn' | 'accent'; detail: string }[]
			};
		}
		const { first: leadText, rest: leadRest } = splitFirstSentence(leadSec.body);
		const tailRows = sections.slice(1).map((sec) => ({
			id: sec.id,
			kicker: sec.kicker,
			tone: sec.tone,
			detail: firstSentence(sec.body)
		}));
		if (leadRest) {
			return {
				leadSec,
				leadText,
				showLeadKicker: false,
				rows: [
					{
						id: leadSec.id,
						kicker: leadSec.kicker,
						tone: leadSec.tone,
						detail: firstSentence(leadRest)
					},
					...tailRows
				]
			};
		}
		return {
			leadSec,
			leadText,
			showLeadKicker: true,
			rows: tailRows
		};
	});
</script>

{#if highlightLayout.leadSec}
	<div class="flex flex-col gap-1">
		{#if highlightLayout.showLeadKicker}
			<span class={['brief-kicker', `tone-${highlightLayout.leadSec.tone}`]}>
				{highlightLayout.leadSec.kicker}
			</span>
		{/if}
		<p class="brief-lead text-[0.9375rem] leading-[1.58] text-[var(--exec-body)]">
			{highlightLayout.leadText}
		</p>
	</div>
{/if}
{#if highlightLayout.rows.length > 0}
	<ul class="dg-list flex flex-col" role="list">
		{#each highlightLayout.rows as row (row.id)}
			<li class={['dg-item grid items-start gap-3', `tone-${row.tone}`]}>
				<span class="dg-ic mt-0.5 inline-flex shrink-0" aria-hidden="true">
					{#if row.tone === 'ok'}
						<CheckCircle2 class="size-4" />
					{:else if row.tone === 'warn'}
						<AlertTriangle class="size-4" />
					{:else}
						<Sparkles class="size-4" />
					{/if}
				</span>
				<div class="flex min-w-0 flex-col gap-0.5">
					<span class="dg-title text-sm font-semibold text-[var(--exec-ink)]">
						{row.kicker}
					</span>
					<span class="dg-detail text-xs leading-relaxed text-[var(--exec-muted)]">
						{row.detail}
					</span>
				</div>
			</li>
		{/each}
	</ul>
{/if}

<style>
	.brief-lead {
		font-weight: 500;
	}
	.brief-kicker {
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.72rem;
		font-weight: 700;
		letter-spacing: 0.1em;
		text-transform: uppercase;
		color: var(--exec-accent-ink);
	}
	.brief-kicker.tone-ok {
		color: var(--exec-ok);
	}
	.brief-kicker.tone-warn {
		color: var(--exec-warn);
	}
	.dg-list {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.dg-item {
		grid-template-columns: 18px minmax(0, 1fr);
		padding: 11px 0;
		border-top: 1px solid var(--exec-border);
	}
	.dg-item:last-child {
		padding-bottom: 2px;
	}
	.dg-ic {
		color: var(--exec-muted);
	}
	.dg-item.tone-ok .dg-ic {
		color: var(--exec-ok);
	}
	.dg-item.tone-warn .dg-ic {
		color: var(--exec-warn);
	}
	.dg-item.tone-accent .dg-ic {
		color: var(--exec-accent-ink);
	}
</style>
