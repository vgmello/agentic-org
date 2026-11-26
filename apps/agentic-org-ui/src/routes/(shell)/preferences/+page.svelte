<script lang="ts">
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Section from '$lib/components/Section.svelte';
	import { Button } from '$lib/components/ui/button';
	import { KeyboardSection, TimeZoneSection } from '$lib/modules/preferences';
	import { pageTitle } from '$lib/nav';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import ChangePasswordDialog from '$lib/platform/auth/ChangePasswordDialog.svelte';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';

	let { data } = $props();
	let changePasswordOpen = $state(false);
	const canChangePassword = $derived(
		appSession.state.kind === 'signed-in' && appSession.authEnabled
	);
	const login = $derived(
		appSession.state.kind === 'signed-in' ? appSession.state.session.account.login : undefined
	);
</script>

<svelte:head><title>{pageTitle('Your preferences')}</title></svelte:head>

<PageHeader title="Your preferences" description="Settings for you, not for the organization." />
<div class="divide-y divide-line-subtle">
	<Section
		title="Tess companion"
		description="Show the floating Tess button in the bottom-right corner. You can always summon Tess with ⌘/ (Ctrl /) even when hidden."
	>
		<div class="flex items-center justify-between gap-4 pt-1">
			<span id="pref-show-tess-label" class="text-sm/6 font-medium text-fg">
				Show Tess on screen
			</span>
			<button
				type="button"
				role="switch"
				aria-checked={uiPrefs.showTessPill}
				aria-labelledby="pref-show-tess-label"
				data-testid="pref-show-tess-toggle"
				onclick={() => uiPrefs.toggleShowTessPill()}
				class={[
					'relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full p-0.5 transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus',
					uiPrefs.showTessPill ? 'bg-primary' : 'bg-surface-overlay ring-1 ring-line-strong'
				]}
			>
				<span
					class={[
						'block size-5 rounded-full transition-transform',
						uiPrefs.showTessPill ? 'translate-x-5 bg-on-emphasis' : 'translate-x-0 bg-fg-muted'
					]}
				></span>
			</button>
		</div>
	</Section>
	<KeyboardSection />
	<TimeZoneSection organizationId={data.connection.organization.id} />
	{#if canChangePassword}
		<Section
			title="Password"
			description={login
				? `Change the password for ${login}. Every other browser session is signed out.`
				: 'Change your sign-in password. Every other browser session is signed out.'}
		>
			<div class="pt-1">
				<Button type="button" variant="outline" onclick={() => (changePasswordOpen = true)}>
					Change password
				</Button>
			</div>
		</Section>
	{/if}
</div>
<ChangePasswordDialog bind:open={changePasswordOpen} />
