<script lang="ts">
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import CircleHelp from '@lucide/svelte/icons/circle-help';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import type { Connection } from '$lib/proto';
	import ShellAccountMenu from './ShellAccountMenu.svelte';
	import { UI_THEMES, uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';

	interface FooterOfficeState {
		label: string;
		tone: 'ok' | 'bad' | 'warn' | 'muted';
		icon: 'online' | 'offline' | 'warn' | 'unknown';
	}

	let {
		connection,
		collapsed = false,
		tessOpen = false,
		offline = false,
		activeTessJobs = 0,
		onToggleTess,
		onNavigate,
		onAccountMenuOpenChange
	}: {
		connection: Connection;
		collapsed?: boolean;
		tessOpen?: boolean;
		offline?: boolean;
		activeTessJobs?: number;
		onToggleTess?: () => void;
		onNavigate?: (event: MouseEvent, href: string) => void;
		onAccountMenuOpenChange?: (open: boolean) => void;
	} = $props();

	const tessStatus = $derived(
		offline
			? { tone: 'off' as const, label: 'Offline' }
			: activeTessJobs > 0
				? { tone: 'busy' as const, label: `Refining (${activeTessJobs})` }
				: { tone: 'ok' as const, label: 'Available' }
	);

	function connectedOfficeStatus(office: Connection['office']): FooterOfficeState {
		switch (office.status) {
			case 'active':
				return { label: 'Online', tone: 'ok', icon: 'online' };
			case 'starting':
				return { label: 'Starting', tone: 'muted', icon: 'unknown' };
			case 'pending':
				return { label: 'Invited', tone: 'muted', icon: 'unknown' };
			case 'unavailable':
				return { label: 'Stopped', tone: 'bad', icon: 'offline' };
			case 'offline':
				return office.role === 'hq'
					? { label: 'Stopped', tone: 'bad', icon: 'offline' }
					: { label: 'Offline', tone: 'muted', icon: 'offline' };
			case 'update_needed':
				return { label: 'Update needed', tone: 'warn', icon: 'warn' };
			default:
				return { label: 'Status unknown', tone: 'muted', icon: 'unknown' };
		}
	}

	const officeStatus = $derived(connectedOfficeStatus(connection.office));
	const officeStatusLabel = $derived(`${connection.office.name} office · ${officeStatus.label}`);
</script>

<div
	class={[
		'exec-sidebar-foot shrink-0 py-2',
		collapsed ? 'flex flex-col items-center gap-1 px-2' : 'px-3'
	]}
>
	{#if onToggleTess}
		<div class={collapsed ? 'mb-1' : 'mb-2'}>
			{#if !collapsed}
				<div class="exec-nav-label px-2 pb-1">Chief of staff</div>
			{/if}
			<Tooltip.Root disabled={!collapsed}>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							onclick={onToggleTess}
							aria-pressed={tessOpen}
							aria-label="Tess — Chief of staff ({tessStatus.label}, ⌘/)"
							class={[
								'exec-tess-contact flex items-center rounded-lg text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid',
								collapsed ? 'size-10 justify-center p-0' : 'w-full gap-2.5 px-2 py-1.5',
								tessOpen && 'is-open'
							]}
						>
							<span
								class="exec-tess-av relative flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold"
							>
								T
								<span class="exec-tess-badge" aria-hidden="true">
									<Sparkles class="size-2.5" />
								</span>
							</span>
							{#if !collapsed}
								<span class="min-w-0 flex-1">
									<span class="exec-tess-name block truncate text-xs/4 font-semibold">Tess</span>
									<span class="exec-tess-status flex items-center gap-1.5 text-[11px]/4">
										<span class={['exec-tess-dot', `is-${tessStatus.tone}`]} aria-hidden="true"
										></span>
										<span class="truncate">{tessStatus.label}</span>
									</span>
								</span>
								<kbd class="exec-sidebar-kbd">⌘/</kbd>
							{/if}
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						side="right"
						sideOffset={6}
						class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
						>Tess · {tessStatus.label} (⌘/)</Tooltip.Content
					>
				</Tooltip.Portal>
			</Tooltip.Root>
		</div>
	{/if}
	{#if !collapsed}
		<div
			class="exec-theme-pills mb-1.5 grid grid-cols-4 gap-0.5 rounded-lg p-1"
			role="group"
			aria-label="Theme"
		>
			{#each UI_THEMES as option (option.id)}
				<button
					type="button"
					onclick={() => uiPrefs.setTheme(option.id)}
					aria-pressed={uiPrefs.theme === option.id}
					title={option.label}
					class={[
						'exec-theme-pill min-h-7 truncate rounded-md px-1 py-0.5 text-[11px] font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid',
						uiPrefs.theme === option.id && 'is-on'
					]}
				>
					{option.shortLabel}
				</button>
			{/each}
		</div>
	{/if}
	<ShellAccountMenu
		{connection}
		compact={collapsed}
		onOpenChange={(next) => onAccountMenuOpenChange?.(next)}
	/>
	<Tooltip.Root disabled={!collapsed}>
		<Tooltip.Trigger>
			{#snippet child({ props })}
				<a
					{...props}
					href={resolve('/settings/offices')}
					onclick={(e) => onNavigate?.(e, '/settings/offices')}
					aria-label={officeStatusLabel}
					class={[
						'exec-foot-link flex items-center rounded-lg text-xs leading-5 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
						collapsed ? 'size-11 justify-center p-0' : 'mt-1 gap-2 px-2 py-1'
					]}
				>
					{#if officeStatus.icon === 'online'}
						<CheckCircle2
							class="size-3.5 shrink-0"
							style="color: var(--exec-ok);"
							aria-hidden="true"
						/>
					{:else if officeStatus.icon === 'offline'}
						<WifiOff
							class="size-3.5 shrink-0"
							style="color: {officeStatus.tone === 'bad'
								? 'var(--exec-bad)'
								: 'var(--exec-muted)'};"
							aria-hidden="true"
						/>
					{:else if officeStatus.icon === 'warn'}
						<CircleAlert
							class="size-3.5 shrink-0"
							style="color: var(--exec-warn);"
							aria-hidden="true"
						/>
					{:else}
						<CircleHelp
							class="size-3.5 shrink-0"
							style="color: var(--exec-muted);"
							aria-hidden="true"
						/>
					{/if}
					{#if !collapsed}
						<span class="min-w-0 [overflow-wrap:anywhere]" style="color: var(--exec-muted);">
							{officeStatusLabel}
						</span>
					{/if}
				</a>
			{/snippet}
		</Tooltip.Trigger>
		<Tooltip.Portal>
			<Tooltip.Content
				side="right"
				sideOffset={6}
				class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
				>{officeStatusLabel}</Tooltip.Content
			>
		</Tooltip.Portal>
	</Tooltip.Root>
</div>

<style>
	.exec-nav-label {
		font-size: var(--exec-nav-label-size);
		line-height: 1.125rem;
		font-weight: 650;
		letter-spacing: var(--exec-nav-label-tracking);
		text-transform: uppercase;
		color: var(--exec-dim);
	}
	.exec-sidebar-foot {
		border-top: 1px solid var(--exec-border);
	}
	.exec-foot-link:hover {
		background: var(--exec-accent-soft);
	}
	.exec-sidebar-kbd {
		font-family: var(--font-mono);
		font-size: 0.65rem;
		line-height: 1;
		padding: 2px 5px;
		border-radius: 5px;
		background: var(--exec-cmdk-kbd-bg);
		color: var(--exec-cmdk-kbd-ink);
		box-shadow: var(--exec-cmdk-kbd-shadow);
	}
	.exec-tess-contact {
		background: transparent;
		color: var(--exec-muted);
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	@media (prefers-reduced-motion: reduce) {
		.exec-tess-contact {
			transition: none;
		}
	}
	.exec-tess-contact:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.exec-tess-contact.is-open {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.exec-tess-av {
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.exec-tess-badge {
		position: absolute;
		right: -2px;
		bottom: -2px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 14px;
		height: 14px;
		border-radius: 999px;
		background: var(--exec-sidebar-bg);
		color: var(--exec-accent-ink);
		box-shadow: 0 0 0 1px var(--exec-border-hi);
	}
	.exec-tess-name {
		color: var(--exec-ink);
	}
	.exec-tess-status {
		color: var(--exec-muted);
	}
	.exec-tess-dot {
		width: 6px;
		height: 6px;
		border-radius: 999px;
		flex: none;
	}
	.exec-tess-dot.is-ok {
		background: var(--tone-ok);
	}
	.exec-tess-dot.is-busy {
		background: var(--tone-accent-hi);
	}
	.exec-tess-dot.is-off {
		background: var(--exec-dim);
	}
	.exec-theme-pills {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.exec-theme-pill {
		color: var(--exec-muted);
	}
	.exec-theme-pill:hover {
		color: var(--exec-ink);
	}
	.exec-theme-pill.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1.5px var(--tone-accent-ink);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.exec-theme-pill {
			min-height: 44px;
		}
	}
	@media (forced-colors: active) {
		.exec-theme-pill.is-on {
			border: 2px solid ButtonText;
		}
	}
</style>
