<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { appSession } from './app-session.svelte';
	import {
		credentialCopy,
		describeChangePasswordFailure,
		PASSWORD_CHANGED_COPY
	} from './credential-copy';
	import { checkRegistration } from './credential-rules';
	import PasswordField from './PasswordField.svelte';

	/**
	 * Change password dialog (server-installation-and-access-ux §4a.2, §4a.4, §4a.7).
	 * Calls POST /api/v1/account/password, keeping the current browser session while signing out
	 * every other session of the account.
	 */
	let {
		open = $bindable(false),
		onchanged
	}: {
		open?: boolean;
		onchanged?: () => void;
	} = $props();

	let currentPassword = $state('');
	let newPassword = $state('');
	let newPasswordAgain = $state('');
	let currentError = $state<string>();
	let newError = $state<string>();
	let generalError = $state<{ message: string; reload?: true }>();
	let busy = $state(false);
	let currentInput = $state<HTMLInputElement | null>(null);
	let newInput = $state<HTMLInputElement | null>(null);

	function resetFields() {
		currentPassword = '';
		newPassword = '';
		newPasswordAgain = '';
		currentError = undefined;
		newError = undefined;
		generalError = undefined;
	}

	$effect(() => {
		if (open) {
			resetFields();
			void tick().then(() => currentInput?.focus());
		}
	});

	onDestroy(resetFields);

	async function validateInputs(): Promise<boolean> {
		if (!currentPassword) {
			currentError = 'Enter your current password.';
			await tick();
			currentInput?.focus();
			return false;
		}
		const problems = checkRegistration('placeholder', newPassword, newPasswordAgain).filter(
			(p) => p.field === 'password'
		);
		if (problems.some((p) => p.rule === 'mismatch')) {
			newPassword = '';
			newPasswordAgain = '';
		}
		if (problems.length > 0) {
			newError = credentialCopy('password', problems[0].rule, 'register');
			await tick();
			newInput?.focus();
			return false;
		}
		return true;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		currentError = undefined;
		newError = undefined;
		generalError = undefined;
		if (!(await validateInputs())) return;
		busy = true;
		try {
			await appSession.controller().changePassword({
				current_password: currentPassword,
				new_password: newPassword
			});
			resetFields();
			open = false;
			toast.success(PASSWORD_CHANGED_COPY);
			onchanged?.();
		} catch (error) {
			const result = describeChangePasswordFailure(error, Date.now());
			if (result.kind === 'current_password') {
				currentPassword = '';
				currentError = result.message;
				await tick();
				currentInput?.focus();
			} else if (result.kind === 'new_password') {
				newError = result.message;
				await tick();
				newInput?.focus();
			} else {
				generalError = { message: result.message, ...(result.reload ? { reload: true } : {}) };
			}
		} finally {
			busy = false;
		}
	}
</script>

<Dialog.Root
	bind:open
	onOpenChange={(next) => {
		if (!next && busy) open = true;
	}}
>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Change password</Dialog.Title>
			<Dialog.Description>
				Changing your password signs you out in every other browser. This session stays signed in.
			</Dialog.Description>
		</Dialog.Header>

		{#if generalError}
			<div
				role="alert"
				class="border-l-4 border-danger bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text-strong"
			>
				<p>{generalError.message}</p>
				{#if generalError.reload}
					<button
						type="button"
						class="mt-1 font-semibold underline"
						onclick={() => location.reload()}>Reload</button
					>
				{/if}
			</div>
		{/if}

		<form class="flex flex-col gap-4" novalidate onsubmit={submit}>
			<PasswordField
				id="change-current-password"
				label="Current password"
				autocomplete="current-password"
				bind:value={currentPassword}
				bind:input={currentInput}
				error={currentError}
				readonly={busy}
			/>
			<PasswordField
				id="change-new-password"
				label="New password (at least 12 characters)"
				autocomplete="new-password"
				bind:value={newPassword}
				bind:input={newInput}
				error={newError}
				readonly={busy}
			/>
			<PasswordField
				id="change-new-password-again"
				label="New password again"
				autocomplete="new-password"
				bind:value={newPasswordAgain}
				readonly={busy}
			/>
			<Dialog.Footer class="mt-2">
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>
					Cancel
				</Button>
				<Button type="submit" disabled={busy}>
					{busy ? 'Changing password…' : 'Change password'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
