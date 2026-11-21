<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Bot from '@lucide/svelte/icons/bot';
	import Building2 from '@lucide/svelte/icons/building-2';
	import Check from '@lucide/svelte/icons/check';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Network from '@lucide/svelte/icons/network';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Workflow from '@lucide/svelte/icons/workflow';
	import { UI_THEMES, uiPrefs, type UiTheme } from '$lib/platform/shell/ui-preferences.svelte';

	let { data } = $props();

	function toggleExecutiveUi() {
		const next = !uiPrefs.executiveUi;
		uiPrefs.setExecutiveUi(next);
		toast.success(
			next
				? 'Executive UI (v2) enabled — open Home or switch themes anytime.'
				: 'Switched back to Classic v1 shell.'
		);
	}

	function selectTheme(theme: UiTheme) {
		uiPrefs.setTheme(theme);
		const found = UI_THEMES.find((t) => t.id === theme);
		toast.success(`Theme switched to ${found?.label ?? theme}.`);
	}
</script>

<svelte:head><title>Settings · Agentic Org</title></svelte:head>

<div class="mx-auto flex w-full max-w-[960px] flex-col gap-6 px-6 py-6 sm:px-8">
	{#if uiPrefs.executiveUi}
		<div>
			<h1 id="shell-page-title" tabindex="-1" class="text-2xl font-semibold tracking-tight focus:outline-none" style="color: var(--exec-ink);">
				Settings
			</h1>
			<p class="mt-0.5 text-sm" style="color: var(--exec-muted);">
				Workspace experience, themes, and configuration for {data.connection.organization.name}.
			</p>
		</div>
	{/if}

	<!-- Feature Flag: Executive UI (v2) -->
	<section class="exec-card flex flex-col gap-4 p-5 sm:p-6" aria-labelledby="settings-flag-h">
		<div class="flex flex-wrap items-start justify-between gap-4">
			<div class="max-w-xl">
				<div class="inline-flex items-center gap-2">
					<Sparkles class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
					<h2 id="settings-flag-h" class="text-base font-semibold" style="color: var(--exec-ink);">
						Executive UI (v2) feature flag
					</h2>
				</div>
				<p class="mt-1 text-sm" style="color: var(--exec-muted);">
					Enable the new Executive Home, Decision Spotlight, floating Command &amp; Org AI bar (⌘K),
					collapsible sidebar, and multi-theme shell. Saved locally per browser so the team can test it safely.
				</p>
			</div>
			<div class="flex items-center gap-3">
				<button
					type="button"
					role="switch"
					aria-checked={uiPrefs.executiveUi}
					aria-labelledby="settings-flag-h"
					data-testid="executive-ui-toggle"
					onclick={toggleExecutiveUi}
					class={['flag-switch relative inline-flex h-7 w-12 shrink-0 items-center rounded-full p-0.5 transition-colors', uiPrefs.executiveUi && 'is-on']}
				>
					<span class={['flag-thumb block size-6 rounded-full bg-white transition-transform', uiPrefs.executiveUi && 'translate-x-5']}></span>
				</button>
				<span class="text-xs font-semibold" style="color: {uiPrefs.executiveUi ? 'var(--exec-ok)' : 'var(--exec-muted)'};">
					{uiPrefs.executiveUi ? 'ON' : 'OFF'}
				</span>
			</div>
		</div>

		{#if uiPrefs.executiveUi}
			<div class="flex flex-wrap items-center gap-3 pt-1">
				<button
					type="button"
					onclick={() => void goto(resolve('/home'))}
					class="exec-cta"
				>
					<span class="exec-cta-fill" aria-hidden="true"></span>
					<span class="exec-cta-inner">
						<span>Open Executive Home</span>
						<ChevronRight class="size-4" aria-hidden="true" />
					</span>
				</button>
				<span class="text-xs" style="color: var(--exec-dim);">
					Tip: you can also toggle via URL with <code>?ui=v2</code> or <code>?ui=v1</code>.
				</span>
			</div>
		{/if}
	</section>

	<!-- Appearance: 3 Themes -->
	<section class="exec-card flex flex-col gap-4 p-5 sm:p-6" aria-labelledby="settings-theme-h">
		<div>
			<h2 id="settings-theme-h" class="text-base font-semibold" style="color: var(--exec-ink);">
				Appearance &amp; Theme
			</h2>
			<p class="mt-0.5 text-sm" style="color: var(--exec-muted);">
				Choose between Frosted Ocean Dark, Daylight Light, or Classic v0 Slate (flat surfaces without shadows).
			</p>
		</div>

		<div class="grid grid-cols-1 gap-3 sm:grid-cols-3" role="radiogroup" aria-labelledby="settings-theme-h">
			{#each UI_THEMES as option (option.id)}
				{@const selected = uiPrefs.theme === option.id}
				<button
					type="button"
					role="radio"
					aria-checked={selected}
					data-testid="theme-option-{option.id}"
					onclick={() => selectTheme(option.id)}
					class={['theme-card flex flex-col items-start gap-2.5 rounded-xl p-4 text-left', selected && 'is-selected']}
				>
					<div class="flex w-full items-center justify-between gap-2">
						<div class={['theme-swatch h-9 w-16 rounded-lg border', `swatch-${option.id}`]} aria-hidden="true">
							<span class="swatch-bar"></span>
						</div>
						{#if selected}
							<span class="flex size-5 items-center justify-center rounded-full" style="background: var(--exec-accent); color: #fff;">
								<Check class="size-3.5" aria-hidden="true" />
							</span>
						{/if}
					</div>
					<div>
						<div class="text-sm font-semibold" style="color: var(--exec-ink);">{option.label}</div>
						<p class="mt-0.5 text-xs leading-relaxed" style="color: var(--exec-muted);">
							{option.description}
						</p>
					</div>
				</button>
			{/each}
		</div>
	</section>

	<!-- Keyboard shortcuts -->
	<section class="exec-card flex flex-wrap items-center justify-between gap-4 p-5 sm:p-6" aria-labelledby="settings-kb-h">
		<div>
			<h2 id="settings-kb-h" class="text-base font-semibold" style="color: var(--exec-ink);">
				Keyboard shortcuts
			</h2>
			<p class="mt-0.5 text-sm" style="color: var(--exec-muted);">
				Press <code>/</code> outside text fields to open Search &amp; Commands (in addition to <code>⌘K</code>).
			</p>
		</div>
		<button
			type="button"
			role="switch"
			aria-checked={uiPrefs.singleKeyShortcuts}
			aria-labelledby="settings-kb-h"
			onclick={() => uiPrefs.setSingleKeyShortcuts(!uiPrefs.singleKeyShortcuts)}
			class={['flag-switch relative inline-flex h-7 w-12 shrink-0 items-center rounded-full p-0.5 transition-colors', uiPrefs.singleKeyShortcuts && 'is-on']}
		>
			<span class={['flag-thumb block size-6 rounded-full bg-white transition-transform', uiPrefs.singleKeyShortcuts && 'translate-x-5']}></span>
		</button>
	</section>

	<!-- Organization management shortcuts -->
	<section class="exec-card flex flex-col gap-3 p-5 sm:p-6" aria-labelledby="settings-org-h">
		<h2 id="settings-org-h" class="text-base font-semibold" style="color: var(--exec-ink);">
			Organization ({data.connection.organization.name})
		</h2>
		<div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
			<a href={resolve('/offices')} class="exec-well flex items-center justify-between gap-3 p-3.5">
				<span class="flex items-center gap-2.5 text-sm font-medium" style="color: var(--exec-ink);">
					<Building2 class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
					<span>Offices ({data.connection.office.name})</span>
				</span>
				<ChevronRight class="size-4" style="color: var(--exec-dim);" aria-hidden="true" />
			</a>
			<a href={resolve('/agents')} class="exec-well flex items-center justify-between gap-3 p-3.5">
				<span class="flex items-center gap-2.5 text-sm font-medium" style="color: var(--exec-ink);">
					<Bot class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
					<span>Agents &amp; setup</span>
				</span>
				<ChevronRight class="size-4" style="color: var(--exec-dim);" aria-hidden="true" />
			</a>
			<a href={resolve('/org-chart')} class="exec-well flex items-center justify-between gap-3 p-3.5">
				<span class="flex items-center gap-2.5 text-sm font-medium" style="color: var(--exec-ink);">
					<Network class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
					<span>Teams &amp; org chart</span>
				</span>
				<ChevronRight class="size-4" style="color: var(--exec-dim);" aria-hidden="true" />
			</a>
			<a href={resolve('/processes')} class="exec-well flex items-center justify-between gap-3 p-3.5">
				<span class="flex items-center gap-2.5 text-sm font-medium" style="color: var(--exec-ink);">
					<Workflow class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
					<span>Processes</span>
				</span>
				<ChevronRight class="size-4" style="color: var(--exec-dim);" aria-hidden="true" />
			</a>
		</div>
	</section>
</div>

<style>
	.flag-switch {
		background: var(--exec-bar-track);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.flag-switch.is-on {
		background: var(--exec-accent);
		box-shadow: none;
	}
	.theme-card {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		transition: box-shadow 0.15s ease, background-color 0.15s ease;
	}
	.theme-card:hover {
		background: var(--exec-surface-hi);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.theme-card.is-selected {
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1.5px var(--exec-accent);
	}
	.theme-swatch {
		position: relative;
		overflow: hidden;
		border-color: var(--exec-border-hi);
	}
	.theme-swatch .swatch-bar {
		position: absolute;
		left: 6px;
		right: 6px;
		bottom: 6px;
		height: 5px;
		border-radius: 999px;
	}
	.swatch-ocean {
		background: linear-gradient(135deg, #060b14 0%, #0f2544 100%);
	}
	.swatch-ocean .swatch-bar {
		background: linear-gradient(90deg, #0284c7, #38bdf8);
	}
	.swatch-light {
		background: linear-gradient(135deg, #f4f7fb 0%, #ffffff 100%);
	}
	.swatch-light .swatch-bar {
		background: linear-gradient(90deg, #0284c7, #0ea5e9);
	}
	.swatch-classic {
		background: #1e2939;
	}
	.swatch-classic .swatch-bar {
		background: #615fff;
	}
</style>
