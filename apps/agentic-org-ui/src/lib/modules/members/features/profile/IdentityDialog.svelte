<!--
  @component
  Edit from the profile (our-org-in-the-app §5, PROF-4): who the employee is, never how it runs. An agent: Name, Role,
  Title, Focus and Handle; a person: Name and Contact. Runtime, model and the managed home are execution settings with
  their own revision: the dialog says so and links to Agent setup instead of offering them. A stale edit (HQ changed
  the record meanwhile) keeps what was typed and takes the latest as the version to send. Mount it in a {#key} that
  changes on each opening: the form starts from the record when it is created.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { resolve } from '$app/paths';
	import { errorClass, helpClass, inputClass, mobileButton } from '$lib/components/classes';
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
	import { toAppError } from '$lib/office/api';
	import { updateAgent, updatePerson } from '$lib/org/actions';
	import { messagingNameError } from '$lib/org/agents';
	import { checkPersonName } from '$lib/org/people-edit';
	import type { OrgSession } from '$lib/org/session';
	import type { Member, OrgSnapshot } from '$lib/proto';
	import {
		agentIdentityChange,
		agentIdentityForm,
		agentIdentityMissing,
		type AgentIdentityForm
	} from './identity';

	let {
		open = $bindable(false),
		member,
		snapshot,
		session,
		organization,
		onCloseAutoFocus,
		ondone
	}: {
		open: boolean;
		member: Member;
		/** The whole organization, former members included: their handles stay taken. */
		snapshot: OrgSnapshot;
		session: OrgSession;
		organization: string;
		onCloseAutoFocus?: (event: Event) => void;
		ondone: (result: { name: string; stale?: boolean }) => void;
	} = $props();

	const ids = $props.id();
	// The caller mounts a fresh dialog for each opening ({#key}), so the form starts from the record once. A reload
	// behind the open dialog (after a stale edit) leaves what was typed alone, while `member`, the latest record, is
	// the baseline and the version sent.
	let form = $state<AgentIdentityForm>(untrack(() => agentIdentityForm(member)));
	let contact = $state(untrack(() => member.contact ?? ''));
	let error = $state<string | undefined>();
	let busy = $state(false);

	const isAgent = $derived(member.kind === 'agent');
	const oldHandle = $derived(member.agent?.messaging_name ?? '');
	const handleError = $derived(
		isAgent && form.handle !== oldHandle
			? messagingNameError(snapshot, form.handle, member.id)
			: undefined
	);
	const personCheck = $derived(
		!isAgent ? checkPersonName(snapshot, form.name, organization, member.id) : undefined
	);
	// The fields start filled from the record, so an empty one is the person's own change: say what it needs.
	const required = (value: string, message: string) =>
		isAgent && value.trim() === '' ? message : undefined;
	const nameError = $derived(
		personCheck && !personCheck.ok ? personCheck.message : required(form.name, 'Enter a name.')
	);
	const invalid = $derived(
		isAgent ? agentIdentityMissing(form) || !!handleError : !personCheck?.ok
	);

	function changeOpen(next: boolean) {
		if (!next && busy) open = true;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (busy || invalid) return;
		const before = member;
		busy = true;
		error = undefined;
		try {
			let name = before.name;
			if (isAgent) {
				const change = agentIdentityChange(before, form);
				if (Object.keys(change).length === 0) return void (open = false);
				await updateAgent(session, before.id, change, before.version);
				name = change.name?.trim() ?? before.name;
			} else {
				if (!personCheck?.ok) return;
				const change: { name?: string; contact?: string } = {};
				if (personCheck.name !== before.name) change.name = personCheck.name;
				if (contact.trim() !== (before.contact ?? '')) change.contact = contact;
				if (Object.keys(change).length === 0) return void (open = false);
				await updatePerson(session, before.id, change);
				name = personCheck.name;
			}
			open = false;
			ondone({ name });
		} catch (e) {
			const failure = toAppError(e);
			if (failure.code === 'conflict') {
				error = `${before.name} changed at HQ while you were editing. Here's the latest; what you typed is kept.`;
				ondone({ name: before.name, stale: true });
			} else error = failure.message;
		} finally {
			busy = false;
		}
	}

	const linkClass =
		'rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
</script>

{#snippet field(
	key: 'name' | 'role' | 'title' | 'focus',
	label: string,
	note: string | undefined,
	err: string | undefined
)}
	<div>
		<Label for="{ids}-{key}" class="flex-wrap gap-x-2 gap-y-0 leading-6"
			>{label}{#if note}<span class="font-normal text-fg-muted">{note}</span>{/if}</Label
		>
		<Input
			id="{ids}-{key}"
			bind:value={form[key]}
			disabled={busy}
			autocomplete="off"
			class="mt-2 {inputClass}"
			aria-invalid={err ? true : undefined}
			aria-describedby={err ? `${ids}-${key}-error` : undefined}
		/>
		{#if err}<p id="{ids}-{key}-error" class={errorClass} role="alert">{err}</p>{/if}
	</div>
{/snippet}

<Dialog bind:open onOpenChange={changeOpen}>
	<DialogContent
		{onCloseAutoFocus}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4 sm:max-w-lg"
	>
		<form class="grid gap-4" onsubmit={save} novalidate>
			<DialogHeader class="pr-8">
				<DialogTitle text="Edit {member.name}" />
				<DialogDescription>
					{#if isAgent}
						Who {member.name} is. How it runs (runtime, model and home folder) is set in
						<a href={resolve('/settings/agents')} class={linkClass}>Agent setup</a>.
					{:else}
						Agents address a person by their name.
					{/if}
				</DialogDescription>
			</DialogHeader>

			{@render field('name', 'Name', undefined, nameError)}
			{#if isAgent}
				<div class="grid gap-4 sm:grid-cols-2">
					{@render field('role', 'Role', undefined, required(form.role, 'Enter a role.'))}
					{@render field('title', 'Title', undefined, required(form.title, 'Enter a title.'))}
				</div>
				{@render field('focus', 'Focus', '(optional; clear it to remove)', undefined)}
				<div>
					<Label for="{ids}-handle">Handle</Label>
					<div class="mt-2 flex items-center gap-x-1">
						<span class="font-mono text-sm/6 text-fg-muted" aria-hidden="true">@</span>
						<Input
							id="{ids}-handle"
							bind:value={form.handle}
							disabled={busy}
							autocomplete="off"
							spellcheck={false}
							class="font-mono {inputClass}"
							aria-invalid={handleError ? true : undefined}
							aria-describedby="{ids}-handle-help"
						/>
					</div>
					<div id="{ids}-handle-help">
						{#if handleError}
							<p class={errorClass} role="alert">{handleError}</p>
						{:else if form.handle !== oldHandle && form.handle !== ''}
							<p class="{helpClass} text-warning-text-strong" data-testid="handle-change-warning">
								Others reach {member.name} as <code class="font-mono">@{oldHandle}</code> today.
								After saving they use <code class="font-mono">@{form.handle}</code>, and
								<code class="font-mono">@{oldHandle}</code> stays reserved.
							</p>
						{:else}
							<p class={helpClass}>How others address {member.name} in messages.</p>
						{/if}
					</div>
				</div>
			{:else}
				<div>
					<Label for="{ids}-contact" class="flex-wrap gap-x-2 gap-y-0 leading-6"
						>Contact <span class="font-normal text-fg-muted">(optional; clear it to remove)</span
						></Label
					>
					<Input
						id="{ids}-contact"
						bind:value={contact}
						disabled={busy}
						autocomplete="off"
						class="mt-2 {inputClass}"
					/>
				</div>
			{/if}

			{#if error}<p class="text-sm/6 text-danger-text [overflow-wrap:anywhere]" role="alert">
					{error}
				</p>{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => (open = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={invalid} aria-busy={busy}
					>{busy ? 'Saving…' : 'Save'}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
