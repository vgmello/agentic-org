<!--
  @component
  The role editor (roles-ux §1): a side sheet for a new role, the HR lead's draft, an edit or a fix. Name, optional
  key and the Markdown body. Openers call `start(mode)`. An edit sends the revision the person saw; a conflict keeps
  every input, shows the newer version and names the effect of saving again ("Save over rev N"). A key that a
  development process in use names is read-only, with the reason (decision 7). Refusals show on their field and
  focus it. Saving with nothing changed says so and adds no revision.
  Back with the sheet open closes the sheet only (app-shell-ux §6) and, like Escape, Close or leaving the page,
  keeps the typed text for when the same editor opens again (§6a keep input). Cancel discards it.
  Layout: from 640 px the fields scroll between a fixed header and footer; below 640 px the whole sheet scrolls as
  one, so at large text the footer never sits over a field (§6a, §2).
-->
<script lang="ts" module>
	import type { Role, RoleDraft } from '../../types';

	export type EditorMode =
		| { kind: 'new'; draft?: RoleDraft; fromHr?: boolean }
		| { kind: 'edit'; role: Role; employeeCount: number };
</script>

<script lang="ts">
	import Callout from '$lib/components/Callout.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
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
		SheetClose,
		SheetContent,
		SheetDescription,
		SheetFooter,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { registerModalBack } from '$lib/platform/shell/modal-back';
	import { rolesApi } from '../../api';
	import {
		NOTHING_TO_SAVE,
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
	import { discardDraft, draftKey, keepDraft, keptDraft } from './kept-drafts';

	let {
		organizationId,
		onsaved,
		onCloseAutoFocus
	}: {
		organizationId: string;
		/** The role as the server wrote it; `created` for a new one. */
		onsaved: (role: Role, created: boolean) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let open = $state(false);
	let mode = $state.raw<EditorMode>({ kind: 'new' });
	let name = $state('');
	let key = $state('');
	let body = $state('');
	let errors = $state<Partial<Record<RoleField, string>>>({});
	let failure = $state<string | undefined>();
	let notice = $state<string | undefined>();
	let busy = $state(false);
	/** The role as it is now, after a conflict: the next save goes over it. */
	let latest = $state.raw<Role | undefined>();
	let conflictMessage = $state('');
	let expectedRevision = $state(0);
	/** Text kept from an earlier close was put back in the fields. */
	let restored = $state(false);
	/** Set by Cancel and by a save: this close leaves nothing to keep. */
	let discarding = false;

	let nameField = $state<HTMLElement | null>(null);
	let keyField = $state<HTMLElement | null>(null);
	let bodyField = $state<HTMLElement | null>(null);
	const fieldFor = (field: RoleField) =>
		field === 'name' ? nameField : field === 'key' ? keyField : bodyField;

	const editing = $derived(mode.kind === 'edit' ? mode.role : undefined);
	const lockedBy = $derived(editing?.keyNamedBy?.length ? editing.keyNamedBy.join(', ') : '');
	const slot = $derived(draftKey(organizationId, editing?.id));

	/** Opens the editor for `next`, with any text kept from the last time this editor closed unsaved. */
	export function start(next: EditorMode) {
		mode = next;
		const role = next.kind === 'edit' ? next.role : undefined;
		const draft = next.kind === 'new' ? next.draft : undefined;
		const slotKey = draftKey(organizationId, role?.id);
		// A fresh HR draft is what the person just asked for: it replaces text kept for a new role.
		if (next.kind === 'new' && next.fromHr) discardDraft(slotKey);
		const kept = keptDraft(slotKey);
		name = kept?.name ?? role?.name ?? draft?.name ?? '';
		key = kept?.key ?? role?.key ?? draft?.key ?? '';
		body = kept?.body ?? role?.bodyMarkdown ?? draft?.bodyMarkdown ?? ROLE_TEMPLATE;
		expectedRevision = kept?.expectedRevision ?? role?.revision ?? 0;
		latest = kept?.latest;
		conflictMessage = kept?.conflictMessage ?? '';
		restored = !!kept;
		errors = {};
		failure = undefined;
		notice = undefined;
		discarding = false;
		open = true;
	}

	/** What was typed differs from what the editor opened with. */
	function typed(): boolean {
		const draft = normalizeDraft({ name, key, bodyMarkdown: body });
		if (editing) return !!latest || !unchanged(editing, draft);
		const initial = mode.kind === 'new' ? mode.draft : undefined;
		return !unchanged(
			{
				name: initial?.name ?? '',
				key: initial?.key,
				bodyMarkdown: initial?.bodyMarkdown ?? ROLE_TEMPLATE
			},
			draft
		);
	}

	function keepOrDiscard() {
		if (!discarding && typed())
			keepDraft(slot, { name, key, body, expectedRevision, latest, conflictMessage });
		else discardDraft(slot);
		discarding = false;
	}

	function cancel() {
		discarding = true;
		open = false;
	}

	// Back with the sheet open closes it only, and focus returns to its opener (§6).
	$effect(() => {
		if (open) return registerModalBack(() => (open = false));
	});
	// Leaving the page with the sheet open keeps the text, as Back does.
	$effect(() => () => {
		if (open) keepOrDiscard();
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
		// A fix starts at the field that was not recognized; otherwise at Name (not the header's Close).
		event.preventDefault();
		const field = editing?.invalid?.field;
		fieldFor(field === 'key' ? 'key' : 'name')?.focus();
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		const draft = normalizeDraft({ name, key, bodyMarkdown: body });
		errors = checkDraft(draft);
		notice = undefined;
		const first = (['name', 'key', 'body'] as const).find((f) => errors[f]);
		if (first) return focusField(first);
		if (editing && !editing.invalid && !latest && unchanged(editing, draft)) {
			// Nothing to save: the sheet stays open and says so; no revision is added.
			notice = NOTHING_TO_SAVE;
			return;
		}
		busy = true;
		failure = undefined;
		try {
			const saved = editing
				? await rolesApi.update(organizationId, editing.id, { ...draft, expectedRevision })
				: await rolesApi.create(organizationId, draft);
			discarding = true;
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
				setTimeout(() => document.getElementById('role-conflict')?.focus(), 0);
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
<Sheet
	bind:open
	onOpenChange={(next) => !next && busy && (open = true)}
	onOpenChangeComplete={(next) => !next && keepOrDiscard()}
>
	<SheetContent
		side="right"
		showCloseButton={false}
		class="w-full gap-0 overflow-y-auto data-[side=right]:w-full data-[side=right]:sm:max-w-2xl min-[640px]:overflow-hidden"
		{onCloseAutoFocus}
		onOpenAutoFocus={openFocus}
	>
		<SheetHeader class="shrink-0 border-b border-white/10">
			<!-- When Close doesn't fit beside the title (narrow, large text) it wraps above it, and the title
				 keeps the full width. -->
			<div class="flex flex-wrap-reverse items-start justify-end gap-x-3 gap-y-2">
				<SheetTitle
					class="min-w-0 flex-1 basis-[200px] text-base/7 font-semibold text-white [overflow-wrap:anywhere]"
					>{title}</SheetTitle
				>
				<SheetClose>
					{#snippet child({ props })}
						<Button
							variant="ghost"
							size="icon"
							class="-my-1 -mr-1.5 shrink-0 text-gray-400 hover:text-white max-[639px]:size-11"
							title="Close"
							{...props}
							><MiniIcon name="xMark" class="size-5" /><span class="sr-only">Close</span></Button
						>
					{/snippet}
				</SheetClose>
			</div>
			<SheetDescription class="text-sm/6 text-gray-400">
				A role describes an employee's work for teammates. It grants no tools, budgets or authority.
			</SheetDescription>
		</SheetHeader>

		<form
			class="flex flex-1 flex-col min-[640px]:min-h-0"
			onsubmit={submit}
			novalidate
			data-testid="role-form"
		>
			<div
				class="flex flex-col gap-y-6 p-4 min-[640px]:min-h-0 min-[640px]:flex-1 min-[640px]:overflow-y-auto"
			>
				{#if restored}
					<Callout data-testid="restored-note">
						<p class="text-sm/6">Your unsaved changes are kept. Cancel discards them.</p>
					</Callout>
				{/if}
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
								<RoleBody body={latest.bodyMarkdown} level={3} />
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
						Markdown. The usual sections are Mission, Responsibilities, Working style and
						Boundaries.
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

				{#if mode.kind === 'edit'}
					<p class="text-sm/6 text-gray-300" data-testid="save-effect">
						Saving makes rev {expectedRevision + 1}.
						{#if mode.employeeCount > 0}
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
			</div>

			<SheetFooter
				class="shrink-0 flex-row flex-wrap items-center justify-end gap-3 border-t border-white/10 px-4 py-4"
			>
				<!-- Always present, so the polite status is announced when it fills; empty, it takes no room. -->
				<p
					role="status"
					class="min-w-0 flex-1 basis-[200px] text-sm/6 text-gray-300 empty:basis-0"
					data-testid="save-notice"
				>
					{notice ?? ''}
				</p>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={cancel}>Cancel</Button
				>
				<Button type="submit" class="min-w-28 {mobileButton}" aria-busy={busy} aria-disabled={busy}
					>{busy ? busyLabel : submitLabel}</Button
				>
			</SheetFooter>
		</form>
	</SheetContent>
</Sheet>
