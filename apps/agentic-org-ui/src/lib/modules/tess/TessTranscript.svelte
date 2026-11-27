<!--
  @component
  Tess conversation transcript, native artifacts, tool progress, and prompt chips.
  Lazy-loaded inside `TessDrawerSurface.svelte` only when the companion opens.
-->
<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { resolve } from '$app/paths';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import { splitCanonicalRefSegments } from './api';
	import TessActionProposalCard from './TessActionProposalCard.svelte';
	import TessEntityList from './TessEntityList.svelte';
	import TessToolProgress from './TessToolProgress.svelte';
	import { TESS_STARTERS, type TessPanelState } from './tess-panel.svelte';

	let {
		panel,
		onSubmitPrompt,
		onArtifactNavigate
	}: {
		panel: TessPanelState;
		onSubmitPrompt: (raw: string) => void;
		onArtifactNavigate: () => void;
	} = $props();

	const askedSet = $derived(new Set(panel.turns.map((t) => t.query.toLowerCase())));
	const remainingStarters = $derived(
		TESS_STARTERS.filter((s) => !askedSet.has(s.toLowerCase())).slice(
			0,
			panel.turns.length > 0 ? 2 : 3
		)
	);
	const latestTurn = $derived(panel.turns.at(-1));
	const latestFollowUps = $derived(
		latestTurn && !latestTurn.streaming && latestTurn.followUps?.length
			? latestTurn.followUps.slice(0, 3)
			: []
	);
	const scrollSignal = $derived(
		`${panel.turns.length}:${latestTurn?.streamText?.length ?? 0}:${latestTurn?.toolCalls?.length ?? 0}:${latestTurn?.blocks?.length ?? 0}`
	);

	function scrollOnTurns(signal: string, turnCount: number): Attachment<HTMLDivElement> {
		return (el) => {
			void signal;
			if (turnCount === 0) return;
			const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
			if (distFromBottom < 120 || el.scrollTop === 0) {
				el.scrollTop = el.scrollHeight;
			}
		};
	}
</script>

<div {@attach scrollOnTurns(scrollSignal, panel.turns.length)} class="tess-body">
	{#if panel.turns.length > 0}
		<div class="tess-thread">
			{#each panel.turns as turn (turn.id)}
				<div class="tess-turn">
					<div class="tess-q">{turn.query}</div>
					<article class="tess-a">
						<div class="tess-a-hd">
							<Sparkles class="size-3" aria-hidden="true" />
							<span>Tess</span>
							{#if turn.mode === 'offline_summary'}
								<span class="tess-a-mode">Offline quick summary · Local data only</span>
							{:else if turn.streaming}
								<span class="tess-a-mode">Live</span>
							{/if}
						</div>

						{#if turn.toolCalls?.length || turn.streaming}
							<TessToolProgress
								calls={turn.toolCalls ?? []}
								streaming={Boolean(turn.streaming)}
								onCancel={turn.streaming ? () => void panel.stopActiveTurn() : undefined}
							/>
						{/if}

						{#if turn.errorMessage}
							<p class="tess-turn-err" role="alert">{turn.errorMessage}</p>
						{/if}

						<div class="tess-a-body">
							{#if turn.mode === 'live'}
								{#if turn.streamText}
									<p class="whitespace-pre-wrap text-[var(--exec-body)]">
										{#each splitCanonicalRefSegments(turn.streamText) as seg, idx (`${idx}-${seg.text}`)}
											{#if seg.href}
												<a
													href={resolve(seg.href as '/')}
													onclick={onArtifactNavigate}
													class="tess-ref-link"
												>
													{seg.text}
												</a>
											{:else}
												{seg.text}
											{/if}
										{/each}
									</p>
								{/if}
							{:else}
								<p class="font-semibold text-[var(--exec-ink,var(--fg))]">
									{turn.answer.headline}
								</p>
								{#if turn.answer.bullets.length > 0}
									<ul class="mt-1.5 flex flex-col gap-1">
										{#each turn.answer.bullets as bullet, idx (`${idx}-${bullet.text}`)}
											<li class="flex flex-wrap items-baseline gap-x-2">
												<span>• {bullet.text}</span>
												{#if bullet.href && bullet.linkLabel}
													<a href={resolve(bullet.href as '/')} onclick={onArtifactNavigate}>
														{bullet.linkLabel}
													</a>
												{/if}
											</li>
										{/each}
									</ul>
								{/if}
							{/if}

							{#if turn.blocks?.length}
								{#each turn.blocks as block, bIdx (`${turn.id}-blk-${bIdx}`)}
									{#if block.kind === 'action_proposal'}
										<TessActionProposalCard
											proposal={block.proposal}
											onApply={(id) => void panel.applyProposal(turn.id, id)}
											onDismiss={(id) => void panel.dismissProposal(turn.id, id)}
											onNavigate={onArtifactNavigate}
										/>
									{:else if block.kind === 'entity_list' || block.kind === 'navigation'}
										<TessEntityList
											title={block.title}
											items={block.items}
											onNavigate={onArtifactNavigate}
										/>
									{:else if block.kind === 'metrics'}
										<TessEntityList
											title={block.title}
											metrics={block.metrics}
											onNavigate={onArtifactNavigate}
										/>
									{/if}
								{/each}
							{/if}
						</div>
					</article>
				</div>
			{/each}
		</div>
	{/if}

	{#if latestFollowUps.length > 0}
		<div class="tess-chips" aria-label="Follow-up prompts">
			{#each latestFollowUps as follow, idx (`${idx}-${follow}`)}
				<button type="button" class="tess-chip" onclick={() => onSubmitPrompt(follow)}>
					<Sparkles class="cmp-chip-ico size-3.5 shrink-0" aria-hidden="true" />
					<span>{follow}</span>
				</button>
			{/each}
		</div>
	{:else if remainingStarters.length > 0}
		<div class="tess-chips" aria-label="Suggested prompts">
			{#each remainingStarters as starter (starter)}
				<button type="button" class="tess-chip" onclick={() => onSubmitPrompt(starter)}>
					<Sparkles class="cmp-chip-ico size-3.5 shrink-0" aria-hidden="true" />
					<span>{starter}</span>
				</button>
			{/each}
		</div>
	{/if}
</div>

<style>
	.tess-body {
		flex: 1 1 auto;
		overflow-y: auto;
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.tess-thread {
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.tess-turn {
		display: grid;
		gap: 6px;
	}
	.tess-q {
		justify-self: end;
		max-width: 86%;
		padding: 7px 11px;
		border-radius: 12px 12px 4px 12px;
		background: var(--exec-tess-q-bg, var(--tone-accent-bg));
		border: 1px solid var(--exec-tess-q-border, var(--tone-accent-ring));
		color: var(--exec-ink, var(--fg));
		font-size: 0.8125rem;
		line-height: 1.45;
	}
	.tess-a {
		padding: 10px 12px;
		border-radius: 10px;
		background: var(--exec-tess-card-bg, var(--exec-well, var(--surface-raised)));
		border: 1px solid var(--exec-tess-card-border, var(--exec-border, var(--line)));
		display: grid;
		gap: 5px;
		font-size: 0.8125rem;
		line-height: 1.5;
		color: var(--exec-body, var(--fg-secondary));
	}
	.tess-a-hd {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		color: var(--tone-accent-ink, var(--link));
		font-size: 0.75rem;
		font-weight: 650;
	}
	.tess-a-mode {
		margin-inline-start: auto;
		font-size: 0.75rem;
		font-weight: 500;
		color: var(--exec-muted, var(--fg-muted));
	}
	.tess-a-body a {
		color: var(--tone-accent-ink, var(--link));
		text-decoration: underline;
		text-underline-offset: 2px;
		font-size: 0.75rem;
		font-weight: 600;
	}
	.tess-turn-err {
		margin-bottom: 6px;
		padding: 7px 10px;
		border-radius: var(--exec-sm-radius, 8px);
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
		box-shadow: inset 0 0 0 1px var(--tone-bad-ring);
		font-size: 0.78rem;
		line-height: 1.4;
	}
	.tess-ref-link {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
		font-weight: 600;
		color: var(--exec-accent-ink);
		text-decoration: underline;
		text-underline-offset: 2px;
	}
	.tess-chips {
		display: flex;
		flex-direction: column;
		gap: 5px;
		margin-top: auto;
	}
	.tess-chip {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 7px 10px;
		border-radius: 9px;
		border: 1px solid var(--exec-tess-card-border, var(--exec-border-hi, var(--line-strong)));
		background: var(--exec-tess-card-bg, var(--exec-well, var(--surface-raised)));
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.8125rem;
		text-align: left;
		cursor: pointer;
		transition:
			border-color 0.14s ease,
			color 0.14s ease,
			background-color 0.14s ease;
	}
	.tess-chip:hover {
		border-color: var(--tone-accent-ring);
		background: var(--tone-accent-bg);
		color: var(--exec-ink, var(--fg));
	}
	.tess-chip :global(.cmp-chip-ico) {
		color: var(--tone-accent-ink, var(--link));
	}
	@media (prefers-reduced-motion: reduce) {
		.tess-chip {
			transition: none;
		}
	}
</style>
