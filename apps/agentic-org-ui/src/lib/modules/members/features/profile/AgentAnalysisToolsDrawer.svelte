<script lang="ts">
	import { resolve } from '$app/paths';
	import { reportedText } from '../../present';
	import type { MemberProfile } from '../../types';

	let {
		profile,
		turnsCount,
		onClose
	}: {
		profile: MemberProfile;
		turnsCount: number;
		onClose: () => void;
	} = $props();
</script>

<div class="af-tools-drawer" id="af-tools-drawer">
	<div class="af-tools-sec">
		<h3 class="af-tools-lbl">Agent context</h3>
		<dl class="af-tools-dl">
			<div>
				<dt>Office</dt>
				<dd>{profile.office ? profile.office.name : 'Unplaced'}</dd>
			</div>
			{#if profile.office}
				<div>
					<dt>Heartbeat</dt>
					<dd>{reportedText(profile.office.observedAt)}</dd>
				</div>
			{/if}
			<div>
				<dt>Health</dt>
				<dd>{profile.supervision?.health ?? 'Not reported'}</dd>
			</div>
		</dl>
	</div>
	<div class="af-tools-sec">
		<h3 class="af-tools-lbl">Current work</h3>
		{#if profile.currentTask}
			<a
				href={resolve(`/work/${encodeURIComponent(profile.currentTask.ref)}` as '/')}
				onclick={onClose}
				class="af-task-link"
			>
				<span class="font-mono">{profile.currentTask.ref}</span>
				<span>{profile.currentTask.title}</span>
			</a>
		{:else}
			<p class="text-xs text-[var(--exec-muted)]">No active task assigned.</p>
		{/if}
	</div>
	<div class="af-tools-sec">
		<h3 class="af-tools-lbl">Recent runs</h3>
		<p class="text-xs text-[var(--exec-muted)] tabular-nums">
			{turnsCount}
			{turnsCount === 1 ? 'session' : 'sessions'} recorded
		</p>
	</div>
</div>

<style>
	.af-tools-drawer {
		width: min(380px, 100%);
		border-radius: 14px;
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border-hi, var(--line-strong));
		padding: 14px 16px;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.af-tools-sec {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding-top: 10px;
		border-top: 1px solid var(--exec-border, var(--line));
	}
	.af-tools-sec:first-child {
		padding-top: 0;
		border-top: 0;
	}
	.af-tools-lbl {
		margin: 0;
		font-size: 0.75rem;
		font-weight: 650;
		letter-spacing: 0.06em;
		text-transform: uppercase;
		color: var(--exec-muted, var(--fg-muted));
	}
	.af-tools-dl {
		margin: 0;
		display: grid;
		gap: 4px;
		font-size: 0.75rem;
	}
	.af-tools-dl > div {
		display: flex;
		justify-content: space-between;
		gap: 8px;
	}
	.af-tools-dl dt {
		color: var(--exec-muted, var(--fg-muted));
	}
	.af-tools-dl dd {
		margin: 0;
		color: var(--exec-body, var(--fg-secondary));
	}
	.af-task-link {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		font-size: 0.8125rem;
		color: var(--tone-accent-ink, var(--link));
		text-decoration: none;
	}
	.af-task-link:hover {
		text-decoration: underline;
	}
</style>
