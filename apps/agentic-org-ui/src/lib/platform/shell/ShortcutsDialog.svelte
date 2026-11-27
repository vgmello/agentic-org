<!--
  @component
  Keyboard Shortcuts Help Overlay (`?` outside text fields, Account menu, or ⌘K) matching POC TASK-145.
  2-column grouped reference (Global, Quick actions, Navigation, Composers) with link to Your preferences.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import { Button } from '$lib/components/ui/button';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';

	let { open = $bindable(false) }: { open: boolean } = $props();

	const isMac = $derived(
		typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/i.test(navigator.platform || '')
	);
	const mod = $derived(isMac ? '⌘' : 'Ctrl ');
	const shiftMod = $derived(isMac ? '⌘⇧' : 'Ctrl Shift ');

	const groups = $derived([
		{
			title: 'Global',
			rows: [
				{ keys: `${mod}K`, desc: 'Command palette & workspace search' },
				{ keys: `${mod}/`, desc: 'Open or minimize Tess' },
				{ keys: `${shiftMod}E`, desc: 'Maximize or restore Tess' },
				{ keys: '?', desc: 'Show keyboard shortcuts' },
				{ keys: 'Esc', desc: 'Close dialog, popover, or minimize Tess' }
			]
		},
		{
			title: 'Quick actions',
			rows: [
				{ keys: 'N', desc: 'New task (Smart composer)' },
				{ keys: '/', desc: 'Focus page search' },
				{ keys: 'A', desc: 'Approve selected Inbox item' }
			]
		},
		{
			title: 'Navigation',
			rows: [
				{ keys: 'S → letter', desc: 'Jump to sidebar section' },
				{ keys: 'M → letter', desc: 'Jump to Inbox category' },
				{ keys: '↑ / ↓', desc: 'Move through lists' },
				{ keys: '← / →', desc: 'Switch tabs & spotlight' }
			]
		},
		{
			title: 'Composers',
			rows: [
				{ keys: `${mod}↵`, desc: 'Send to Tess or post comment' },
				{ keys: `${shiftMod}↵`, desc: 'Create task as-is (skip Tess)' }
			]
		}
	]);
</script>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="kbd-help-overlay fixed inset-0 z-90" />
		<Dialog.Content
			data-testid="shortcuts-help-dialog"
			class="kbd-help-dialog fixed top-1/2 left-1/2 z-100 w-[min(620px,calc(100vw-32px))] -translate-x-1/2 -translate-y-1/2 rounded-xl p-6 focus:outline-none"
		>
			<Dialog.Title class="text-base/7 font-semibold text-[var(--exec-ink,var(--fg))]">
				Keyboard shortcuts
			</Dialog.Title>
			<Dialog.Description class="mt-1 text-sm/5 text-[var(--exec-muted,var(--fg-muted))]">
				{uiPrefs.singleKeyShortcuts
					? 'Single-key shortcuts (N, /, S, M, A) are on.'
					: 'Single-key shortcuts are currently off in Your preferences.'}
			</Dialog.Description>

			<div class="kbd-help-grid mt-4">
				{#each groups as group (group.title)}
					<section class="kbd-help-grp">
						<h3>{group.title}</h3>
						<dl>
							{#each group.rows as row (row.keys)}
								<div class="kbd-help-row">
									<dt><kbd>{row.keys}</kbd></dt>
									<dd>{row.desc}</dd>
								</div>
							{/each}
						</dl>
					</section>
				{/each}
			</div>

			<div class="mt-6 flex flex-wrap justify-end gap-2.5">
				<Button variant="outline" href={resolve('/preferences')} onclick={() => (open = false)}>
					Preferences
				</Button>
				<Button type="button" onclick={() => (open = false)}>Done</Button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<style>
	:global(.kbd-help-overlay) {
		background: var(--exec-scrim-bg, color-mix(in srgb, var(--surface-sunken) 75%, transparent));
		-webkit-backdrop-filter: blur(6px);
		backdrop-filter: blur(6px);
	}
	:global(.kbd-help-dialog) {
		background: var(--exec-notif-bg, var(--exec-cmdk-menu-bg, var(--surface)));
		-webkit-backdrop-filter: var(--exec-notif-blur, var(--exec-cmdk-menu-blur, none));
		backdrop-filter: var(--exec-notif-blur, var(--exec-cmdk-menu-blur, none));
		border: 1px solid var(--exec-notif-border, var(--exec-border-hi, var(--line-strong)));
		box-shadow: var(--exec-notif-shadow, var(--exec-cmdk-menu-shadow, none));
		color: var(--exec-ink, var(--fg));
	}
	.kbd-help-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 16px 22px;
	}
	.kbd-help-grp h3 {
		margin: 0 0 8px;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 650;
		letter-spacing: 0.06em;
		text-transform: uppercase;
	}
	.kbd-help-grp dl {
		display: grid;
		gap: 7px;
		margin: 0;
	}
	.kbd-help-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		font-size: 0.8125rem;
	}
	.kbd-help-row dt {
		order: 2;
		flex: none;
	}
	.kbd-help-row dd {
		order: 1;
		margin: 0;
		color: var(--exec-body, var(--fg-secondary));
	}
	.kbd-help-row kbd {
		display: inline-flex;
		align-items: center;
		padding: 2px 7px;
		border-radius: 5px;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		background: var(--exec-well, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.75rem;
		font-weight: 600;
	}
	@media (max-width: 560px) {
		.kbd-help-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
