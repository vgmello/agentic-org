<!--
  @component
  Section management dialogs (Add section, Rename section, Move document) for DocsTab.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Dialog from '$lib/components/ui/dialog';
	import { errorClass, inputClass, mobileButton, selectClass } from '$lib/components/classes';
	import { toAppError } from '$lib/proto';
	import { docsApi } from '../../api';
	import { checkSectionPath } from '../../present';
	import type { DocSection, ProjectDocumentRow } from '../../types';

	let {
		uid,
		organizationId,
		projectId,
		sections,
		onreload
	}: {
		uid: string;
		organizationId: string;
		projectId: string;
		sections: readonly DocSection[];
		onreload: () => Promise<void>;
	} = $props();

	let addSectionOpen = $state(false);
	let newSectionPath = $state('');
	let newSectionTitle = $state('');
	let newSectionSummary = $state('');
	let sectionBusy = $state(false);
	let sectionError = $state<string | undefined>();

	let renameTarget = $state<DocSection | null>(null);
	let renameTitle = $state('');
	let renameSummary = $state('');

	let moveTarget = $state<ProjectDocumentRow | null>(null);
	let moveSectionPath = $state('');
	let moveBusy = $state(false);
	let moveError = $state<string | undefined>();

	export function openAddSection() {
		newSectionPath = '';
		newSectionTitle = '';
		newSectionSummary = '';
		sectionError = undefined;
		addSectionOpen = true;
	}

	export function openRenameSection(section: DocSection) {
		renameTarget = section;
		renameTitle = section.title;
		renameSummary = section.summary;
		sectionError = undefined;
	}

	export function openMoveDoc(doc: ProjectDocumentRow) {
		moveTarget = doc;
		moveSectionPath = doc.sectionPath ?? sections[0]?.path ?? '';
		moveError = undefined;
	}

	async function submitAddSection(event: SubmitEvent) {
		event.preventDefault();
		if (sectionBusy) return;
		const pathErr = checkSectionPath(newSectionPath);
		if (pathErr) {
			sectionError = pathErr;
			return;
		}
		if (!newSectionTitle.trim()) {
			sectionError = 'Enter a title for the section.';
			return;
		}
		sectionBusy = true;
		sectionError = undefined;
		try {
			await docsApi.addSection(organizationId, projectId, {
				path: newSectionPath.trim().replace(/^\/+|\/+$/g, ''),
				title: newSectionTitle.trim(),
				summary: newSectionSummary.trim()
			});
			addSectionOpen = false;
			toast.success(`Added section ${newSectionTitle.trim()}.`);
			await onreload();
		} catch (e) {
			sectionError = toAppError(e).message;
		} finally {
			sectionBusy = false;
		}
	}

	async function submitRenameSection(event: SubmitEvent) {
		event.preventDefault();
		if (!renameTarget || sectionBusy) return;
		if (!renameTitle.trim()) {
			sectionError = 'Enter a title for the section.';
			return;
		}
		sectionBusy = true;
		sectionError = undefined;
		try {
			await docsApi.renameSection(organizationId, projectId, renameTarget.path, {
				title: renameTitle.trim(),
				summary: renameSummary.trim()
			});
			const saved = renameTitle.trim();
			renameTarget = null;
			toast.success(`Updated section ${saved}.`);
			await onreload();
		} catch (e) {
			sectionError = toAppError(e).message;
		} finally {
			sectionBusy = false;
		}
	}

	async function submitMoveDoc(event: SubmitEvent) {
		event.preventDefault();
		if (!moveTarget || moveBusy || !moveSectionPath) return;
		moveBusy = true;
		moveError = undefined;
		try {
			await docsApi.moveDocument(organizationId, projectId, moveTarget.id, {
				sectionPath: moveSectionPath,
				expectedRevision: moveTarget.revision
			});
			const title = moveTarget.title;
			moveTarget = null;
			toast.success(`Moved ${title}.`);
			await onreload();
		} catch (e) {
			moveError = toAppError(e).message;
		} finally {
			moveBusy = false;
		}
	}
</script>

<!-- Add section dialog -->
<Dialog.Root bind:open={addSectionOpen}>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<Dialog.Header>
			<Dialog.Title text="Add documentation section" />
			<Dialog.Description>
				Adds a folder section to the project's documentation index.
			</Dialog.Description>
		</Dialog.Header>
		<form class="flex flex-col gap-4" onsubmit={submitAddSection}>
			<div>
				<Label for="{uid}-sec-path" class="mb-1 block text-sm/6 font-medium text-fg">
					Folder path
				</Label>
				<Input
					id="{uid}-sec-path"
					placeholder="architecture"
					bind:value={newSectionPath}
					class={inputClass}
				/>
				<p class="mt-1 text-xs/5 text-fg-muted">
					Relative to <span class="font-mono">docs/</span>. Lowercase letters, digits, hyphens and
					slashes.
				</p>
			</div>
			<div>
				<Label for="{uid}-sec-title" class="mb-1 block text-sm/6 font-medium text-fg">Title</Label>
				<Input
					id="{uid}-sec-title"
					placeholder="Architecture"
					bind:value={newSectionTitle}
					class={inputClass}
				/>
			</div>
			<div>
				<Label for="{uid}-sec-summary" class="mb-1 block text-sm/6 font-medium text-fg">
					Summary
				</Label>
				<Input
					id="{uid}-sec-summary"
					placeholder="One-line reason to open this section."
					bind:value={newSectionSummary}
					class={inputClass}
				/>
			</div>
			{#if sectionError}
				<p class={errorClass} role="alert">{sectionError}</p>
			{/if}
			<Dialog.Footer>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={sectionBusy}
					onclick={() => (addSectionOpen = false)}
				>
					Cancel
				</Button>
				<Button type="submit" class={mobileButton} disabled={sectionBusy}>
					{sectionBusy ? 'Adding section…' : 'Add section'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<!-- Rename section dialog -->
<Dialog.Root
	open={renameTarget !== null}
	onOpenChange={(open) => {
		if (!open) renameTarget = null;
	}}
>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<Dialog.Header>
			<Dialog.Title text="Rename section" />
			<Dialog.Description>
				Updates the section title and summary in the documentation index without moving files.
			</Dialog.Description>
		</Dialog.Header>
		<form class="flex flex-col gap-4" onsubmit={submitRenameSection}>
			<div>
				<Label for="{uid}-rename-title" class="mb-1 block text-sm/6 font-medium text-fg">
					Title
				</Label>
				<Input id="{uid}-rename-title" bind:value={renameTitle} class={inputClass} />
			</div>
			<div>
				<Label for="{uid}-rename-summary" class="mb-1 block text-sm/6 font-medium text-fg">
					Summary
				</Label>
				<Input id="{uid}-rename-summary" bind:value={renameSummary} class={inputClass} />
			</div>
			{#if sectionError}
				<p class={errorClass} role="alert">{sectionError}</p>
			{/if}
			<Dialog.Footer>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={sectionBusy}
					onclick={() => (renameTarget = null)}
				>
					Cancel
				</Button>
				<Button type="submit" class={mobileButton} disabled={sectionBusy}>
					{sectionBusy ? 'Saving…' : 'Save section'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<!-- Move document dialog -->
<Dialog.Root
	open={moveTarget !== null}
	onOpenChange={(open) => {
		if (!open) moveTarget = null;
	}}
>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<Dialog.Header>
			<Dialog.Title text="Move document" />
			<Dialog.Description>
				Moving a document updates its path in the documentation tree while preserving its stable ID,
				history, issued versions, and links.
			</Dialog.Description>
		</Dialog.Header>
		<form class="flex flex-col gap-4" onsubmit={submitMoveDoc}>
			<div>
				<Label for="{uid}-move-sec" class="mb-1 block text-sm/6 font-medium text-fg">
					Destination section
				</Label>
				<select id="{uid}-move-sec" bind:value={moveSectionPath} class={selectClass}>
					{#each sections as sec (sec.path)}
						<option value={sec.path}>{sec.title} (docs/{sec.path})</option>
					{/each}
				</select>
			</div>
			{#if moveError}
				<p class={errorClass} role="alert">{moveError}</p>
			{/if}
			<Dialog.Footer>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={moveBusy}
					onclick={() => (moveTarget = null)}
				>
					Cancel
				</Button>
				<Button type="submit" class={mobileButton} disabled={moveBusy}>
					{moveBusy ? 'Moving…' : 'Move document'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
