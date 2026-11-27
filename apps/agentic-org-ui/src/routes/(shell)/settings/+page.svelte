<script lang="ts">
	import { toast } from 'svelte-sonner';
	import Check from '@lucide/svelte/icons/check';
	import GeneralPanel from '$lib/modules/settings/features/general/GeneralPanel.svelte';
	import { pageTitle } from '$lib/nav';
	import { UI_THEMES, uiPrefs, type UiTheme } from '$lib/platform/workspace/ui-preferences.svelte';

	let { data } = $props();

	const isCozyTheme = $derived(uiPrefs.theme === 'cozy');

	function selectTheme(theme: UiTheme) {
		uiPrefs.setTheme(theme);
		const found = UI_THEMES.find((t) => t.id === theme);
		toast.success(`Theme switched to ${found?.label ?? theme}.`);
	}

	function handleThemeRadioKeydown(event: KeyboardEvent, index: number) {
		const count = UI_THEMES.length;
		let nextIndex: number;
		switch (event.key) {
			case 'ArrowRight':
			case 'ArrowDown':
				nextIndex = (index + 1) % count;
				break;
			case 'ArrowLeft':
			case 'ArrowUp':
				nextIndex = (index - 1 + count) % count;
				break;
			case 'Home':
				nextIndex = 0;
				break;
			case 'End':
				nextIndex = count - 1;
				break;
			default:
				return;
		}
		event.preventDefault();
		const nextTheme = UI_THEMES[nextIndex];
		if (!nextTheme) return;
		selectTheme(nextTheme.id);
		const group = (event.currentTarget as HTMLElement | null)?.closest('[role="radiogroup"]');
		group?.querySelector<HTMLElement>(`[data-testid="theme-option-${nextTheme.id}"]`)?.focus();
	}

	function toggleCozyHighContrast() {
		const next = !uiPrefs.cozyHighContrast;
		uiPrefs.setCozyHighContrast(next);
		toast.success(
			next ? 'Cozy Glass high contrast enabled.' : 'Cozy Glass high contrast disabled.'
		);
	}
</script>

<svelte:head><title>{pageTitle('General', 'Organization settings')}</title></svelte:head>

{#key data.connection.organization.id}<GeneralPanel
		organizationId={data.connection.organization.id}
	/>{/key}

<section
	class="border-t border-line-subtle px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8"
	aria-labelledby="settings-theme-h"
>
	<div class="max-w-4xl">
		<h3 id="settings-theme-h" class="text-base/7 font-semibold text-fg">Appearance &amp; Theme</h3>
		<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">
			Choose between Ocean, Classic, Graphite, Graphite Light, Cozy Glass, Report (Modern 2D Day),
			or Report Night. Saved on this browser.
		</p>

		<div
			class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3"
			role="radiogroup"
			aria-labelledby="settings-theme-h"
		>
			{#each UI_THEMES as option, index (option.id)}
				{@const selected = uiPrefs.theme === option.id}
				<button
					type="button"
					role="radio"
					aria-checked={selected}
					tabindex={selected ? 0 : -1}
					data-testid="theme-option-{option.id}"
					onclick={() => selectTheme(option.id)}
					onkeydown={(event) => handleThemeRadioKeydown(event, index)}
					class={[
						'theme-card flex min-h-11 flex-col items-start gap-2.5 rounded-xl p-4 text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus',
						selected && 'is-selected'
					]}
				>
					<div class="flex w-full items-center justify-between gap-2">
						<div
							class={['theme-swatch h-9 w-16 rounded-lg border', `swatch-${option.id}`]}
							aria-hidden="true"
						>
							<span class="swatch-bar"></span>
						</div>
						{#if selected}
							<span class="theme-check flex size-5 items-center justify-center rounded-full">
								<Check class="size-3.5" aria-hidden="true" />
							</span>
						{/if}
					</div>
					<div>
						<div class="text-sm/6 font-semibold text-fg">{option.label}</div>
						<p class="mt-0.5 text-xs/5 text-fg-muted">
							{option.description}
						</p>
					</div>
				</button>
			{/each}
		</div>

		<div
			class={[
				'contrast-card mt-4 flex flex-wrap items-center justify-between gap-4 rounded-xl px-4 py-3.5',
				isCozyTheme && 'is-active-theme'
			]}
		>
			<div class="min-w-0 flex-1">
				<div class="flex flex-wrap items-center gap-2">
					<span id="cozy-contrast-label" class="text-sm/6 font-semibold text-fg">
						Cozy Glass high contrast
					</span>
					<span class="contrast-badge rounded-full px-2 py-0.5 text-xs font-medium">
						{isCozyTheme ? 'Active theme' : 'Applies in Cozy Glass'}
					</span>
				</div>
				<p id="cozy-contrast-help" class="mt-0.5 text-xs/5 text-fg-muted">
					Makes frosted glass surfaces more opaque and strengthens border and text contrast when
					Cozy Glass is active.
				</p>
			</div>
			<button
				type="button"
				role="switch"
				aria-checked={uiPrefs.cozyHighContrast}
				aria-labelledby="cozy-contrast-label"
				aria-describedby="cozy-contrast-help"
				data-testid="cozy-high-contrast-toggle"
				onclick={toggleCozyHighContrast}
				class={[
					'contrast-switch relative inline-flex h-11 min-w-11 shrink-0 items-center justify-center rounded-lg px-1 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid',
					uiPrefs.cozyHighContrast && 'is-on'
				]}
			>
				<span class="contrast-track relative inline-flex h-6 w-11 items-center rounded-full p-0.5">
					<span class="contrast-thumb block size-5 rounded-full"></span>
				</span>
			</button>
		</div>
	</div>
</section>

<style>
	.theme-card {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		transition:
			box-shadow 0.15s ease,
			background-color 0.15s ease;
	}
	.theme-card:hover {
		background: var(--exec-surface-hi);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.theme-card.is-selected {
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1.5px var(--tone-accent);
	}
	.theme-check {
		background: var(--exec-accent);
		color: var(--exec-on-accent);
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
	.contrast-card {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.contrast-card.is-active-theme {
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.contrast-badge {
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.contrast-switch {
		cursor: pointer;
	}
	.contrast-track {
		background: var(--exec-surface-hi);
		box-shadow: inset 0 0 0 1px var(--field-border);
		transition:
			background-color 0.16s ease,
			box-shadow 0.16s ease;
	}
	.contrast-switch.is-on .contrast-track {
		background: var(--exec-accent);
		box-shadow: inset 0 0 0 1px var(--tone-accent);
	}
	.contrast-thumb {
		background: var(--exec-ink);
		transform: translateX(0);
		transition:
			transform 0.16s ease,
			background-color 0.16s ease;
	}
	.contrast-switch.is-on .contrast-thumb {
		background: var(--exec-on-accent);
		transform: translateX(20px);
	}
	@media (prefers-reduced-motion: reduce) {
		.contrast-track,
		.contrast-thumb {
			transition: none;
		}
	}
	@media (forced-colors: active) {
		.theme-card.is-selected,
		.contrast-track {
			border: 2px solid ButtonText;
		}
		.contrast-thumb,
		.contrast-switch.is-on .contrast-thumb,
		.contrast-switch[aria-checked='true'] .contrast-thumb {
			background: ButtonText;
		}
	}
</style>
