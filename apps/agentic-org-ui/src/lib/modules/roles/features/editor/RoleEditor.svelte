<!--
  @component
  The role editor (roles-ux §1): a side sheet for a new role, the HR lead's draft, an edit or a fix. Name, optional
  key and the Markdown body. An edit sends the revision the person saw; a conflict keeps every input, shows the
  newer version and names the effect of saving again ("Save over rev N"). A key that a development process in use
  names is read-only, with the reason (decision 7). Refusals show on their field and focus it.
-->
<script lang="ts" module>
	import type { Role, RoleDraft } from '../../types';

	export type EditorMode =
		| { kind: 'new'; draft?: RoleDraft; fromHr?: boolean }
		| { kind: 'edit'; role: Role; employeeCount: number };
</script>

<script lang="ts">
	import { untrack } from 'svelte';
	import Callout from '$lib/components/Callout.svelte';
	import {
		errorClass,
		helpClass,
		inputClass,
		mobileButton,
		textareaClass
	} from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetFooter,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { rolesApi } from '../../api';
	import {
		ROLE_TEMPLATE,
		checkDraft,
		conflictLatest,
		employeesText,
		invalidText,
		normalizeDraft,
		refusedField,
		unchanged
	} from '../../present';
	import type { RoleField } from '../../types';
	import RoleBody from '../role-page/RoleBody.svelte';

	let {
		open = $bindable(false),
		organizationId,
		mode,
		onsaved,
		onCloseAutoFocus
	}: {
		open: boolean;
		organizationId: string;
		mode: EditorMode;
		/** The role as the server wrote it; `created` for a new one. */
		onsaved: (role: Role, created: boolean) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	const editing = $derived(mode.kind === 'edit' ? mode.role : undefined);
	const lockedBy = $derived(editing?.keyNamedBy?.length ? editing.keyNamedBy.join(', ') : '');

	let name = $state('');
	let key = $state('');
	let body = $state('');
	let errors = $state<Partial<Record<RoleField, string>>>({});
	let failure = $state<string | undefined>();
	let busy = $state(false);
	/** The role as it is now, after a conflict: the next save goes over it. */
	let latest = $state.raw<Role | undefined>();
	let conflictMessage = $state('');
	let expectedRevision = $state(0);

	let nameField = $state<HTMLElement | null>(null);
	let keyField = $state<HTMLElement | null>(null);
	let bodyField = $state<HTMLElement | null>(null);
	const fieldFor = (field: RoleField) =>
		field === 'name' ? nameField : field === 'key' ? keyField : bodyField;

	// Each time the sheet opens it starts from the mode's values (only `open` triggers this).
	$effect(() => {
		if (!open) return;
		untrack(() => {
			const role = mode.kind === 'edit' ? mode.role : undefined;
			const draft = mode.kind === 'new' ? mode.draft : undefined;
			name = role?.name ?? draft?.name ?? '';
			key = role?.key ?? draft?.key ?? '';
			body = role?.bodyMarkdown ?? draft?.bodyMarkdown ?? ROLE_TEMPLATE;
			expectedRevision = role?.revision ?? 0;
			errors = {};
			failure = undefined;
			latest = undefined;
			conflictMessage = '';
		});
	});

	const title = $derived(
		mode.kind === 'new'
			? 'New role'
			: mode.role.invalid
				? `Fix ${mode.role.name}`
				: `Edit ${mode.role.name}`
	);
	const submitLabel = $derived(
		latest ? `Save over rev ${latest.revision}` : mode.kind === 'new' ? 'Create role' : 'Save role'
	);
	const busyLabel = $derived(mode.kind === 'new' ? 'Creating…' : 'Saving…');

	function focusField(field: RoleField) {
		setTimeout(() => fieldFor(field)?.focus(), 0);
	}

	function openFocus(event: Event) {
		// A fix starts at the field that was not recognized; otherwise the sheet's first field.
		const field = editing?.invalid?.field;
		if (field === 'name' || field === 'key') {
			event.preventDefault();
			fieldFor(field)?.focus();
		}
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		const draft = normalizeDraft({ name, key, bodyMarkdown: body });
		errors = checkDraft(draft);
		const first = (['name', 'key', 'body'] as const).find((f) => errors[f]);
		if (first) return focusField(first);
		if (editing && !editing.invalid && !latest && unchanged(editing, draft)) {
			// Nothing to save: closing adds no revision.
			open = false;
			return;
		}
		busy = true;
		failure = undefined;
		try {
			const saved = editing
				? await rolesApi.update(organizationId, editing.id, { ...draft, expectedRevision })
				: await rolesApi.create(organizationId, draft);
			open = false;
			onsaved(saved, !editing);
		} catch (e) {
			const newer = conflictLatest(e);
			const field = refusedField(e);
			const message = toAppError(e).message;
			if (newer) {
				latest = newer;
				expectedRevision = newer.revision;
				conflictMessage = message;
				// The notice is at the top of the sheet, away from Save: bring it into view and read it.
				setTimeout(() => {
					const notice = document.getElementById('role-conflict');
					notice?.focus();
				}, 0);
			} else if (field) {
				errors = { ...errors, [field]: message };
				focusField(field);
			} else failure = message;
		} finally {
			busy = false;
		}
	}
</script>

<!-- A click outside or Escape doesn't dismiss the sheet while the role is being saved. -->
<Sheet bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<SheetContent
		side="right"
		class="w-full overflow-y-auto data-[side=right]:w-full data-[side=right]:sm:max-w-2xl"
		{onCloseAutoFocus}
		onOpenAutoFocus={openFocus}
	>
		<SheetHeader class="pr-14">
			<SheetTitle class="text-base/7 font-semibold text-white [overflow-wrap:anywhere]"
				>{title}</SheetTitle
			>
			<SheetDescription class="text-sm/6 text-gray-400">
				A role describes an employee's work for teammates. It grants no tools, budgets or authority.
			</SheetDescription>
		</SheetHeader>

		<form class="flex flex-1 flex-col gap-y-6 px-4" onsubmit={submit} novalidate>
			{#if mode.kind === 'new' && mode.fromHr}
				<Callout data-testid="hr-draft-note">
					<p class="text-sm/6">
						Drafted by the HR lead. Review it, then create the role: nothing is created until you
						do.
					</p>
				</Callout>
			{/if}
			{#if editing?.invalid}
				<Callout data-testid="invalid-note">
					<p class="text-sm/6 font-semibold text-red-300">
						{invalidText(editing.name, editing.invalid.field)}
					</p>
					<p class="mt-1 text-sm/6">
						Saving writes the file again with details the office recognizes.
					</p>
				</Callout>
			{/if}
			{#if latest}
				<Callout role="alert" data-testid="conflict">
					<p
						id="role-conflict"
						tabindex="-1"
						class="text-sm/6 font-semibold text-white focus:outline-none"
					>
						{conflictMessage}
					</p>
					<details open class="mt-2">
						<summary
							class="cursor-pointer rounded-sm text-sm/6 font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:py-2.5"
							>Newer version: rev {latest.revision}, {latest.name}</summary
						>
						<div class="mt-2 border-l-2 border-white/10 pl-3">
							<RoleBody body={latest.bodyMarkdown} level={4} />
						</div>
					</details>
				</Callout>
			{/if}

			<div>
				<Label for="role-name" class="block text-sm/6 font-medium text-white">Name</Label>
				<div class="mt-2">
					<Input
						bind:ref={nameField}
						id="role-name"
						bind:value={name}
						autocomplete="off"
						required
						aria-invalid={errors.name ? true : undefined}
						aria-describedby={errors.name ? 'role-name-error' : undefined}
						oninput={() => errors.name && (errors = { ...errors, name: undefined })}
						class={inputClass}
					/>
				</div>
				{#if errors.name}<p id="role-name-error" class={errorClass}>{errors.name}</p>{/if}
			</div>

			<div>
				<Label for="role-key" class="block text-sm/6 font-medium text-white"
					>Key <span class="font-normal text-gray-400">(optional)</span></Label
				>
				<div class="mt-2">
					<Input
						bind:ref={keyField}
						id="role-key"
						bind:value={key}
						readonly={!!lockedBy}
						autocomplete="off"
						autocapitalize="none"
						spellcheck={false}
						aria-invalid={errors.key ? true : undefined}
						aria-describedby="role-key-help{errors.key ? ' role-key-error' : ''}"
						oninput={() => errors.key && (errors = { ...errors, key: undefined })}
						class="{inputClass} font-mono read-only:text-gray-300 read-only:outline-white/10"
					/>
				</div>
				<p id="role-key-help" class={helpClass}>
					{#if lockedBy}
						{lockedBy} uses this key, so it can't be changed or removed.
					{:else}
						Development processes find roles by key. Lowercase letters, digits and hyphens, unique
						in the organization.
					{/if}
				</p>
				{#if errors.key}<p id="role-key-error" class={errorClass}>{errors.key}</p>{/if}
			</div>

			<div>
				<Label for="role-body" class="block text-sm/6 font-medium text-white">Text</Label>
				<p id="role-body-help" class="mt-1 text-xs/5 text-gray-400">
					Markdown. The usual sections are Mission, Responsibilities, Working style and Boundaries.
				</p>
				<textarea
					bind:this={bodyField}
					id="role-body"
					bind:value={body}
					rows="16"
					spellcheck="true"
					aria-invalid={errors.body ? true : undefined}
					aria-describedby="role-body-help{errors.body ? ' role-body-error' : ''}"
					oninput={() => errors.body && (errors = { ...errors, body: undefined })}
					class="{textareaClass} mt-2 font-mono"></textarea>
				{#if errors.body}<p id="role-body-error" class={errorClass}>{errors.body}</p>{/if}
			</div>

			{#if editing}
				<p class="text-sm/6 text-gray-300" data-testid="save-effect">
					Saving makes rev {expectedRevision + 1}.
					{#if mode.kind === 'edit' && mode.employeeCount > 0}
						The {employeesText(mode.employeeCount)} with this role read the new text.
					{:else}
						No employees have this role yet.
					{/if}
				</p>
			{/if}

			{#if failure}
				<p class="text-sm/6 text-red-300 [overflow-wrap:anywhere]" role="alert">
					{failure}
				</p>
			{/if}

			<SheetFooter
				class="sticky bottom-0 -mx-4 mt-auto flex-row flex-wrap justify-end gap-3 border-t border-white/10 bg-popover px-4 py-4"
			>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => (open = false)}>Cancel</Button
				>
				<Button type="submit" class="min-w-28 {mobileButton}" aria-busy={busy} aria-disabled={busy}
					>{busy ? busyLabel : submitLabel}</Button
				>
			</SheetFooter>
		</form>
	</SheetContent>
</Sheet>
