<script lang="ts">
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Sparkles from '@lucide/svelte/icons/sparkles';

	let {
		decisionId,
		backgroundContext,
		auditChecks,
		open = false,
		onToggle,
		onAsk
	}: {
		decisionId: string;
		backgroundContext: string;
		auditChecks: readonly string[];
		open: boolean;
		onToggle: () => void;
		onAsk: () => void;
	} = $props();
</script>

<div class="flex flex-col gap-2">
	<div class="spot-ctx-row flex flex-wrap items-center justify-between gap-2.5">
		<button
			type="button"
			onclick={onToggle}
			aria-expanded={open}
			aria-controls="panel-spot-ctx-{decisionId}"
			class="ctx-toggle inline-flex w-fit items-center gap-1.5 rounded-md px-1.5 text-xs font-semibold"
		>
			<ChevronRight
				class={['size-3.5 transition-transform', open && 'rotate-90']}
				aria-hidden="true"
			/>
			<span>{open ? 'Hide background context' : 'Read background context'}</span>
		</button>
		<button
			type="button"
			onclick={onAsk}
			class="spot-ai-ask inline-flex min-h-9 items-center gap-1.5 px-2.5 text-xs font-semibold"
		>
			<Sparkles class="size-3.5" aria-hidden="true" />
			<span>Ask about this</span>
		</button>
	</div>
	{#if open}
		<div id="panel-spot-ctx-{decisionId}" class="ctx-body px-3.5 py-2.5 text-xs leading-relaxed">
			<p>{backgroundContext}</p>
			{#if auditChecks.length > 0}
				<div
					class="ctx-checks mt-2.5 flex flex-wrap gap-x-3.5 gap-y-1.5 pt-2.5"
					role="list"
					aria-label="Decision checks"
				>
					{#each auditChecks as chk (chk)}
						<span class="inline-flex items-center gap-1.5" role="listitem">
							<CheckCircle2 class="size-3.5 shrink-0 text-[var(--exec-ok)]" aria-hidden="true" />
							<span>{chk}</span>
						</span>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</div>

<style>
	.ctx-toggle {
		min-height: 36px;
		color: var(--exec-accent-ink);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.ctx-toggle:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.spot-ai-ask {
		border-radius: var(--exec-sm-radius);
		color: var(--exec-accent-ink);
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.spot-ai-ask:hover {
		background: var(--exec-accent);
		color: var(--exec-on-accent);
		box-shadow: inset 0 0 0 1px var(--exec-accent);
	}
	.ctx-body {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-well);
		color: var(--exec-body);
		box-shadow: inset 2px 0 0 var(--tone-accent);
	}
	.ctx-checks {
		border-top: 1px solid var(--exec-border);
		color: var(--exec-muted);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.ctx-toggle,
		.spot-ai-ask {
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.ctx-toggle,
		.spot-ai-ask {
			transition: none;
		}
	}
</style>
