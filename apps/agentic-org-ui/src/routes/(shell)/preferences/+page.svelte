<script lang="ts">
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Section from '$lib/components/Section.svelte';
	import { Button } from '$lib/components/ui/button';
	import { KeyboardSection, TimeZoneSection } from '$lib/modules/preferences';
	import { pageTitle } from '$lib/nav';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import ChangePasswordDialog from '$lib/platform/auth/ChangePasswordDialog.svelte';

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
