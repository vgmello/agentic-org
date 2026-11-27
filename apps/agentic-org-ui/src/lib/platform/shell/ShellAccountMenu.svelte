<!--
  @component
  The footer's "You · Owner" account menu (app-shell-ux §0): Your preferences, Switch organization, About,
  and Sign out only while a signed-in session supports it. It is about the viewer's account, not an
  employee record. `compact` renders the rail's icon-only trigger.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import { icons } from '$lib/components/icons';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { signOutFlow } from '$lib/platform/auth/sign-out-flow.svelte';
	import type { Connection } from '$lib/proto';
	import AboutDialog from './AboutDialog.svelte';
	import { useShellCommand } from '$lib/platform/workspace/shell-command';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';

	let {
		connection,
		compact = false,
		onOpenChange
	}: {
		connection: Connection;
		compact?: boolean;
		onOpenChange?: (open: boolean) => void;
	} = $props();

	const shellCommand = useShellCommand();
	let aboutOpen = $state(false);
	const canSignOut = $derived(appSession.state.kind === 'signed-in' && appSession.authEnabled);
	const isLocalAuthless = $derived(
		appSession.state.kind === 'signed-in' && !appSession.authEnabled
	);

	async function signOut() {
		await signOutFlow.request();
		if (appSession.state.kind === 'signed-out')
			await goto(resolve('/sign-in?note=signed-out'), { replaceState: true });
	}
	const focusRing =
		'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus';
</script>

<DropdownMenu.Root onOpenChange={(next) => onOpenChange?.(next)}>
	<Tooltip.Root disabled={!compact}>
		<Tooltip.Trigger>
			{#snippet child({ props: tooltipProps })}
				<DropdownMenu.Trigger {...tooltipProps}>
					{#snippet child({ props })}
						{#if compact}
							<button
								{...props}
								type="button"
								aria-label="You · Owner, account menu"
								class="flex size-12 items-center justify-center rounded-md text-fg-muted hover:bg-surface-raised hover:text-fg {focusRing}"
							>
								<svg
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									stroke-width="1.5"
									aria-hidden="true"
									class="size-6"
								>
									<path d={icons.user} stroke-linecap="round" stroke-linejoin="round" />
								</svg>
							</button>
						{:else}
							<button
								{...props}
								type="button"
								class="flex min-h-11 w-full items-center gap-x-3 rounded-md px-2 py-2 text-left text-sm/6 hover:bg-surface-raised {focusRing}"
							>
								<svg
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									stroke-width="1.5"
									aria-hidden="true"
									class="size-6 shrink-0 text-fg-muted"
								>
									<path d={icons.user} stroke-linecap="round" stroke-linejoin="round" />
								</svg>
								<span class="min-w-0 flex-1 font-semibold text-fg"
									>You <span class="font-normal text-fg-muted">· Owner</span></span
								>
								<MiniIcon name="chevronUpDown" class="size-5 text-fg-muted" />
							</button>
						{/if}
					{/snippet}
				</DropdownMenu.Trigger>
			{/snippet}
		</Tooltip.Trigger>
		<Tooltip.Portal
			><Tooltip.Content
				role="tooltip"
				side="right"
				sideOffset={12}
				class="z-[100] rounded-md bg-tooltip px-3 py-2 text-xs text-on-emphasis"
				>You · Owner</Tooltip.Content
			></Tooltip.Portal
		>
	</Tooltip.Root>
	<DropdownMenu.Content
		side={compact ? 'right' : 'top'}
		align="start"
		class="w-60 bg-surface ring-line"
	>
		<DropdownMenu.Item
			class="min-h-9 max-[639px]:min-h-11"
			onSelect={() => goto(resolve('/preferences'))}>Your preferences</DropdownMenu.Item
		>
		<DropdownMenu.CheckboxItem
			checked={uiPrefs.showTessPill}
			onCheckedChange={(checked) => uiPrefs.setShowTessPill(Boolean(checked))}
			class="min-h-9 max-[639px]:min-h-11"
		>
			Show Tess on screen
		</DropdownMenu.CheckboxItem>
		{#if shellCommand?.openShortcuts}
			<DropdownMenu.Item
				class="flex min-h-9 items-center justify-between gap-2 max-[639px]:min-h-11"
				onSelect={() => shellCommand.openShortcuts?.()}
			>
				<span>Keyboard shortcuts</span>
				<kbd
					class="rounded border border-line-strong px-1.5 py-0.5 font-mono text-xs text-fg-muted"
					aria-hidden="true">?</kbd
				>
			</DropdownMenu.Item>
		{/if}
		<DropdownMenu.Separator class="bg-surface-overlay" />
		<DropdownMenu.Item
			class="min-h-9 max-[639px]:min-h-11"
			onSelect={() => goto(resolve('/welcome'))}>Switch organization</DropdownMenu.Item
		>
		<DropdownMenu.Item class="min-h-9 max-[639px]:min-h-11" onSelect={() => (aboutOpen = true)}
			>About</DropdownMenu.Item
		>
		{#if canSignOut}
			<DropdownMenu.Separator class="bg-surface-overlay" />
			<DropdownMenu.Item
				class="min-h-9 max-[639px]:min-h-11"
				disabled={signOutFlow.signingOut}
				onSelect={signOut}>{signOutFlow.signingOut ? 'Signing out…' : 'Sign out'}</DropdownMenu.Item
			>
		{:else if isLocalAuthless}
			<DropdownMenu.Separator class="bg-surface-overlay" />
			<DropdownMenu.Item
				class="min-h-9 max-[639px]:min-h-11"
				onSelect={() => goto(resolve('/settings/security'))}
				>Local mode · Authentication off</DropdownMenu.Item
			>
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>
<AboutDialog bind:open={aboutOpen} {connection} />
