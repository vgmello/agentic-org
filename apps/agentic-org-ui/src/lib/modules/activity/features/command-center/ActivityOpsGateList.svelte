<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import type { ReleaseGateRow } from './assembly-model';

	let { gates }: { gates: readonly ReleaseGateRow[] } = $props();
</script>

<ul class="ops-list">
	{#each gates as gate (gate.id)}
		<li>
			<a
				href={resolve(gate.href as '/inbox')}
				class="ops-gate-link min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
			>
				<span class="ops-gate-label">
					{#if gate.tone === 'ok'}
						<CheckCircle2
							class="size-3.5 shrink-0"
							style="color: var(--exec-ok);"
							aria-hidden="true"
						/>
					{:else if gate.tone === 'bad'}
						<AlertTriangle
							class="size-3.5 shrink-0"
							style="color: var(--exec-bad);"
							aria-hidden="true"
						/>
					{:else if gate.tone === 'warn'}
						<AlertTriangle
							class="size-3.5 shrink-0"
							style="color: var(--exec-warn);"
							aria-hidden="true"
						/>
					{:else}
						<span class="ops-gate-dot" aria-hidden="true"></span>
					{/if}
					<span class="ops-gate-text">{gate.label}</span>
				</span>
				<span class="ops-gate-status tabular-nums" data-tone={gate.tone}>
					{gate.statusLabel}
				</span>
			</a>
		</li>
	{/each}
</ul>

<style>
	.ops-list {
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.ops-gate-link {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 4px 0;
		color: var(--exec-body);
		font-size: 0.8125rem;
		text-decoration: none;
		min-width: 0;
	}

	.ops-gate-link:hover {
		color: var(--tone-accent-ink);
	}

	.ops-gate-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	.ops-gate-label {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.ops-gate-text {
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.ops-gate-dot {
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: var(--fg-secondary);
		flex-shrink: 0;
	}

	.ops-gate-status {
		font-size: 0.75rem;
		font-weight: 600;
		white-space: nowrap;
		flex-shrink: 0;
	}

	.ops-gate-status[data-tone='ok'] {
		color: var(--exec-ok);
	}

	.ops-gate-status[data-tone='warn'] {
		color: var(--exec-warn);
	}

	.ops-gate-status[data-tone='bad'] {
		color: var(--exec-bad);
	}

	.ops-gate-status[data-tone='neutral'] {
		color: var(--fg-secondary);
	}
</style>
