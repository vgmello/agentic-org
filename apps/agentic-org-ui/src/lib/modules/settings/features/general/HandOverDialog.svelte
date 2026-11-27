<!--
  @component
  Hand over the administrator role (SIGN-39): pick another account by the person it is tied to, confirm with your own password.
  Both accounts change in one operation, so there is never a moment with two administrators or none.
-->
<script lang="ts">
	import { onDestroy } from 'svelte';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { settingsApi } from '../../api';
	import {
		errorClass,
		helpClass,
		inputClass,
		mobileButton,
		selectClass
	} from '$lib/components/classes';
	import type { GeneralSettings } from '../../types';

	let {
		open = $bindable(false),
		general,
		ondone,
		onCloseAutoFocus
	}: {
		open: boolean;
		general: GeneralSettings;
		ondone: (next: GeneralSettings) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let chosen = $state('');
	let password = $state('');
	let busy = $state(false);
	let error = $state<string | undefined>();
	let passwordInput = $state<HTMLInputElement | null>(null);
	const person = $derived(general.handOverCandidates.find((c) => c.memberId === chosen));

	function resetOnOpen() {
		chosen = '';
		password = '';
		error = undefined;
	}

	onDestroy(() => {
		password = '';
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || !person) return;
		if (password === '') {
			error = 'Enter your password to confirm.';
			passwordInput?.focus();
			return;
		}
		busy = true;
		error = undefined;
		try {
			const next = await settingsApi.handOverAdministrator(
				general.organization.id,
				person.memberId,
				password
			);
			open = false;
			password = '';
			ondone(next);
			toast.success(`${person.name} is now this instance's administrator.`);
		} catch (e) {
			error = toAppError(e).message;
			password = '';
			passwordInput?.focus();
		} finally {
			busy = false;
		}
	}
</script>

<Dialog
	bind:open
	onOpenChange={(next) => {
		if (!next) {
			if (busy) open = true;
			else password = '';
		}
	}}
>
	<DialogContent
		onOpenAutoFocus={resetOnOpen}
		{onCloseAutoFocus}
		class="max-h-[calc(100dvh-2rem)] overflow-y-auto"
	>
		<form class="grid gap-6" onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle text="Hand over administrator" />
				<DialogDescription class="[overflow-wrap:anywhere]">
					{#if person}Make {person.name} ({person.username}) this instance's administrator? You'll
						keep your account and your organizations, but you won't be able to create organizations
						or collect recovery codes.{:else}The administrator creates organizations and collects
						recovery codes. There is always exactly one.{/if}
				</DialogDescription>
			</DialogHeader>
			{#if general.handOverCandidates.length === 0}
				<p class="text-sm/6 text-fg-secondary" data-testid="no-candidates">
					Nobody else has an account yet. Invite a person to sign in under <a
						href={resolve('/settings/access')}
						class="font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-focus"
						>Members and access</a
					> first.
				</p>
			{:else}
				<div>
					<Label for="handover-person" class="block text-sm/6 font-medium text-fg"
						>New administrator</Label
					>
					<div class="mt-2">
						<select
							id="handover-person"
							bind:value={chosen}
							class={selectClass}
							aria-describedby="handover-person-help"
						>
							<option value="">Choose a person…</option>
							{#each general.handOverCandidates as c (c.memberId)}<option value={c.memberId}
									>{c.name} ({c.username})</option
								>{/each}
						</select>
					</div>
					<p id="handover-person-help" class={helpClass}>
						People who have an account in one of your organizations.
					</p>
				</div>
				<div>
					<Label for="handover-password" class="block text-sm/6 font-medium text-fg"
						>Your password</Label
					>
					<div class="mt-2">
						<Input
							bind:ref={passwordInput}
							id="handover-password"
							type="password"
							autocomplete="current-password"
							bind:value={password}
							disabled={!person}
							aria-invalid={error ? true : undefined}
							aria-describedby={error ? 'handover-error' : undefined}
							class={inputClass}
						/>
					</div>
					{#if error}<p id="handover-error" class={errorClass} role="alert">{error}</p>{/if}
				</div>
			{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => {
						if (busy) return;
						password = '';
						open = false;
					}}>Cancel</Button
				>
				{#if general.handOverCandidates.length > 0}
					<Button
						type="submit"
						class={mobileButton}
						disabled={!person}
						aria-busy={busy}
						aria-disabled={busy}>{busy ? 'Handing over…' : 'Hand over'}</Button
					>
				{/if}
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
