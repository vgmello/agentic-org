<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import XCircle from '@lucide/svelte/icons/x-circle';
	import type { ProjectBentoCard } from '$lib/home/model';

	interface Props {
		projects: readonly ProjectBentoCard[];
		attentionCount: number;
	}

	let { projects, attentionCount }: Props = $props();
</script>

<section class="scorecard-sec flex flex-col gap-3.5" aria-labelledby="exec-projects-heading">
	<div class="flex items-baseline justify-between gap-3">
		<div class="flex items-baseline gap-2.5">
			<h2 id="exec-projects-heading" class="text-[15px] font-semibold text-[var(--exec-ink)]">
				Project Delivery Scorecard
			</h2>
			<span class="text-xs text-[var(--exec-muted)]">
				{projects.length} active{attentionCount > 0 ? ` · ${attentionCount} need attention` : ''}
			</span>
		</div>
		<a
			href={resolve('/projects')}
			class="inline-flex items-center gap-0.5 text-xs font-semibold text-[var(--exec-accent-ink)]"
		>
			<span>All projects</span>
			<ChevronRight class="size-3.5" aria-hidden="true" />
		</a>
	</div>

	<div class="scorecard-grid grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3">
		{#each projects.slice(0, 6) as proj (proj.id)}
			<article class="proj-card flex flex-col gap-3.5">
				<div class="flex items-center justify-between gap-2">
					<div class="flex min-w-0 items-baseline gap-2">
						<a
							href={resolve(proj.workHref as '/projects')}
							class="truncate text-[15px] font-semibold text-[var(--exec-ink)] hover:underline"
						>
							{proj.name}
						</a>
						{#if proj.key}
							<span class="key-tag font-mono text-[11px]">{proj.key}</span>
						{/if}
					</div>
					<span class={['health-pill inline-flex items-center gap-1', proj.statusPillClass]}>
						{#if proj.healthTone === 'bad'}
							<XCircle class="size-3" aria-hidden="true" />
						{:else if proj.healthTone === 'warn'}
							<AlertTriangle class="size-3" aria-hidden="true" />
						{:else}
							<CheckCircle2 class="size-3" aria-hidden="true" />
						{/if}
						<span>{proj.health}</span>
					</span>
				</div>

				<a href={resolve(proj.workHref as '/projects')} class="flex flex-col gap-1.5">
					<div
						class="flex items-baseline justify-between text-xs text-[var(--exec-muted)] tabular-nums"
					>
						<span>
							{proj.doneCount} of {proj.totalCount} done{proj.dueLabel
								? ` · Due ${proj.dueLabel}`
								: ''}
						</span>
						<strong class="text-sm text-[var(--exec-ink)]">{proj.pct}%</strong>
					</div>
					<div class="bar-track bg-bar-track" aria-hidden="true">
						<span class="bar-{proj.healthTone}" style:width="{proj.pct}%"></span>
					</div>
				</a>

				<div
					class={[
						'blocker-box flex min-h-[56px] items-center justify-between gap-2.5 px-3 py-2 text-xs',
						`tone-${proj.blockerTone}`
					]}
				>
					<span class="min-w-0 flex-1 leading-snug">{proj.blockerText}</span>
					<a
						href={resolve(proj.actionHref as '/work')}
						class="unblock-btn inline-flex min-h-9 shrink-0 items-center gap-0.5 rounded-md px-2.5 py-1 text-[11px] font-semibold"
					>
						<span>{proj.actionLabel}</span>
						<ChevronRight class="size-3" aria-hidden="true" />
					</a>
				</div>

				<div
					class="proj-foot mt-auto flex items-center justify-between gap-2 pt-3 text-xs text-[var(--exec-muted)]"
				>
					<div class="flex items-center">
						{#each proj.memberInitials as init, idx (`${proj.id}-${idx}-${init}`)}
							<span
								class="avatar-stack inline-flex size-6 items-center justify-center rounded-full text-[9px] font-bold text-[var(--exec-ink)]"
							>
								{init}
							</span>
						{/each}
						{#if proj.extraMembers > 0}
							<span class="ml-1.5 text-[11px]">+{proj.extraMembers}</span>
						{/if}
					</div>
					<a
						href={resolve(proj.workHref as '/projects')}
						class="inline-flex items-center gap-0.5 hover:underline"
					>
						<span>Lead {proj.leadName}</span>
						<ChevronRight class="size-3" aria-hidden="true" />
					</a>
				</div>
			</article>
		{/each}
	</div>
</section>

<style>
	.scorecard-sec {
		padding: var(--exec-section-pad-y);
		border-bottom: var(--exec-section-border);
	}
	.scorecard-grid {
		gap: var(--exec-section-gap);
	}
	.proj-card {
		background: var(--exec-panel-bg);
		backdrop-filter: var(--exec-panel-blur);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-proj-pad-sm);
	}
	.proj-card:hover {
		background: var(--exec-panel-bg-hover);
		box-shadow: var(--exec-panel-shadow-hover);
	}
	@media (max-width: 767px) {
		.proj-card + .proj-card {
			border-top: var(--exec-col-divider);
		}
	}
	@media (min-width: 768px) and (max-width: 1023px) {
		.proj-card {
			padding: var(--exec-proj-pad);
			border-left: var(--exec-col-divider);
		}
		.proj-card:nth-child(2n + 1) {
			border-left: 0;
			padding-left: var(--exec-proj-edge-pad);
		}
		.proj-card:nth-child(2n) {
			padding-right: var(--exec-proj-edge-pad);
		}
		.proj-card:nth-child(n + 3) {
			border-top: var(--exec-col-divider);
			padding-top: var(--exec-proj-row-pt-md);
			margin-top: var(--exec-proj-row-mt-md);
		}
	}
	@media (min-width: 1024px) {
		.proj-card {
			padding: var(--exec-proj-pad);
			border-left: var(--exec-col-divider);
		}
		.proj-card:nth-child(3n + 1) {
			border-left: 0;
			padding-left: var(--exec-proj-edge-pad);
		}
		.proj-card:nth-child(3n) {
			padding-right: var(--exec-proj-edge-pad);
		}
		.proj-card:nth-child(n + 4) {
			border-top: var(--exec-col-divider);
			padding-top: var(--exec-proj-row-pt);
			margin-top: var(--exec-proj-row-mt);
		}
	}
	.key-tag {
		padding: 1px 6px;
		border-radius: 5px;
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.health-pill {
		padding: 2px 8px;
		border-radius: var(--exec-pill-radius);
		font-size: 0.7rem;
		font-weight: 600;
		white-space: nowrap;
	}
	.bar-track {
		display: flex;
		gap: 2px;
		height: var(--exec-bar-height);
		border-radius: var(--exec-bar-radius);
		overflow: hidden;
	}
	.bar-track > span {
		display: block;
		height: 100%;
		border-radius: var(--exec-bar-radius);
	}
	.bar-ok {
		background: var(--tone-ok-bar);
	}
	.bar-warn {
		background: var(--tone-warn-bar);
	}
	.bar-bad {
		background: var(--tone-bad-bar);
	}
	.blocker-box {
		border-radius: var(--exec-inset-radius);
		color: var(--exec-body);
	}
	.blocker-box.tone-bad {
		background: var(--exec-bad-bg);
		box-shadow: inset 0 0 0 1px var(--exec-bad-ring);
	}
	.blocker-box.tone-warn {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
	}
	.blocker-box.tone-sky {
		background: var(--exec-sky-bg);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
	}
	.blocker-box.tone-ok {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	.unblock-btn {
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.unblock-btn {
			min-height: 44px;
		}
	}
	.proj-foot {
		border-top: 1px solid var(--exec-border);
	}
	.avatar-stack {
		background: var(--exec-avatar-bg);
		margin-left: -6px;
		box-shadow: 0 0 0 2px var(--exec-avatar-ring);
	}
	.avatar-stack:first-child {
		margin-left: 0;
	}
</style>
