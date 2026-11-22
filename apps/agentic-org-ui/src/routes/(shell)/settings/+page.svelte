<script lang="ts">
	import { toast } from 'svelte-sonner';
	import Check from '@lucide/svelte/icons/check';
	import GeneralPanel from '$lib/modules/settings/features/general/GeneralPanel.svelte';
	import { pageTitle } from '$lib/nav';
	import { UI_THEMES, uiPrefs, type UiTheme } from '$lib/platform/shell/ui-preferences.svelte';

	let { data } = $props();

	function selectTheme(theme: UiTheme) {
		uiPrefs.setTheme(theme);
		const found = UI_THEMES.find((t) => t.id === theme);
		toast.success(`Theme switched to ${found?.label ?? theme}.`);
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
			Choose between Frosted Ocean, Classic Slate, Report (Modern 2D Day), or Report Night. Saved on
			this browser.
		</p>

		<div
			class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2"
			role="radiogroup"
			aria-labelledby="settings-theme-h"
		>
			{#each UI_THEMES as option (option.id)}
				{@const selected = uiPrefs.theme === option.id}
				<button
					type="button"
					role="radio"
					aria-checked={selected}
					data-testid="theme-option-{option.id}"
					onclick={() => selectTheme(option.id)}
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
</style>
