<!--
  @component
  Current decision edit warning dialog (DEC-6) and Issue version dialog (organization-data 18) for DocumentPage.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Dialog from '$lib/components/ui/dialog';
	import { errorClass, inputClass, mobileButton } from '$lib/components/classes';
	import { toAppError } from '$lib/proto';
	import { docsApi } from '../../api';
	import { newDocHref } from '../../present';
	import type { ProjectDocumentDetail } from '../../types';

	let {
		uid,
		organizationId,
		projectId,
		doc,
		decisionWarnOpen = $bindable(false),
		onEditAnyway,
		onIssued
	}: {
		uid: string;
		organizationId: string;
		projectId: string;
		doc: ProjectDocumentDetail;
		decisionWarnOpen: boolean;
		onEditAnyway: (doc: ProjectDocumentDetail) => void;
		onIssued: () => Promise<void>;
	} = $props();

	let issueOpen = $state(false);
	let issueNote = $state('');
	let issueDerivedIds = $state<string[]>([]);
	let issueBusy = $state(false);
	let issueError = $state<string | undefined>();

	export function openIssueDialog() {
		issueNote = '';
		issueDerivedIds = [];
		issueError = undefined;
		issueOpen = true;
	}

	function toggleDerivedSource(key: string) {
		issueDerivedIds = issueDerivedIds.includes(key)
			? issueDerivedIds.filter((k) => k !== key)
			: [...issueDerivedIds, key];
	}

	async function submitIssueVersion(event: SubmitEvent) {
		event.preventDefault();
		if (issueBusy) return;
		issueBusy = true;
		issueError = undefined;
		try {
			const derivedFrom = doc.derivableSources
				.filter((s) => issueDerivedIds.includes(`${s.documentId}:${s.version}`))
				.map(({ documentId, version }) => ({ documentId, version }));
			const updated = await docsApi.issueVersion(organizationId, projectId, doc.id, {
				note: issueNote.trim(),
				derivedFrom,
				expectedRevision: doc.revision
			});
			issueOpen = false;
			toast.success(`Issued version v${updated.version} of ${updated.title}.`);
			await onIssued();
		} catch (e) {
			issueError = toAppError(e).message;
		} finally {
			issueBusy = false;
		}
	}
</script>

<!-- Current decision edit warning dialog (DEC-6 / §4) -->
<Dialog.Root bind:open={decisionWarnOpen}>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<Dialog.Header>
			<Dialog.Title text="Edit a current decision?" />
			<Dialog.Description>
				Decisions are a record. Supersede it if the decision changed, or edit only to fix a mistake.
			</Dialog.Description>
		</Dialog.Header>
		<Dialog.Footer>
			<Button
				type="button"
				variant="outline"
				class={mobileButton}
				onclick={() => (decisionWarnOpen = false)}
			>
				Cancel
			</Button>
			<Button
				type="button"
				variant="outline"
				class={mobileButton}
				onclick={() => {
					decisionWarnOpen = false;
					void goto(resolve(newDocHref(projectId, 'decision', { supersedes: doc.id }) as '/'));
				}}
			>
				Supersede with a new decision
			</Button>
			<Button
				type="button"
				class={mobileButton}
				onclick={() => {
					decisionWarnOpen = false;
					onEditAnyway(doc);
				}}
			>
				Edit anyway
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Issue version dialog (organization-data 18) -->
<Dialog.Root bind:open={issueOpen}>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<Dialog.Header>
			<Dialog.Title text="Issue version v{(doc.version ?? 0) + 1}" />
			<Dialog.Description>
				Fixes the current working text into an immutable version in docs-history and sets the
				document status to Current.
			</Dialog.Description>
		</Dialog.Header>
		<form class="flex flex-col gap-4" onsubmit={submitIssueVersion}>
			<div>
				<Label for="{uid}-issue-note" class="mb-1 block text-sm/6 font-medium text-fg">
					Version note
				</Label>
				<Input
					id="{uid}-issue-note"
					placeholder="What changed in this issued version?"
					bind:value={issueNote}
					class={inputClass}
				/>
			</div>

			{#if doc.derivableSources.length > 0}
				<fieldset class="flex flex-col gap-2">
					<legend class="text-sm/6 font-medium text-fg">Derived from (optional)</legend>
					<p class="text-xs/5 text-fg-muted">
						Record issued versions of other documents this version follows.
					</p>
					{#each doc.derivableSources as src (`${src.documentId}:${src.version}`)}
						{@const srcKey = `${src.documentId}:${src.version}`}
						<label class="flex items-center gap-2 text-sm/6 text-fg-body">
							<input
								type="checkbox"
								checked={issueDerivedIds.includes(srcKey)}
								onchange={() => toggleDerivedSource(srcKey)}
								class="size-4 rounded border-field-border accent-primary"
							/>
							<span>
								{src.key ? `${src.key} · ` : ''}{src.title}
								<span class="font-mono text-xs/5 text-fg-muted">v{src.version}</span>
							</span>
						</label>
					{/each}
				</fieldset>
			{/if}

			{#if issueError}
				<p class={errorClass} role="alert">{issueError}</p>
			{/if}
			<Dialog.Footer>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={issueBusy}
					onclick={() => (issueOpen = false)}
				>
					Cancel
				</Button>
				<Button type="submit" class={mobileButton} disabled={issueBusy}>
					{issueBusy ? 'Issuing…' : `Issue v${(doc.version ?? 0) + 1}`}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
