<!--
  @component
  Link-a-task dialog with cross-feature move confirmation (FTR-12) for FeaturePage.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import * as Dialog from '$lib/components/ui/dialog';
	import { errorClass, mobileButton, selectClass } from '$lib/components/classes';
	import { toAppError } from '$lib/proto';
	import { featuresApi } from '../../api';
	import { featureRef } from '../../present';
	import type { EligibleTaskChoice, FeatureDetail } from '../../types';

	let {
		uid,
		organizationId,
		projectId,
		projectName,
		feature,
		onlinked
	}: {
		uid: string;
		organizationId: string;
		projectId: string;
		projectName: string;
		feature: FeatureDetail;
		onlinked: () => Promise<void>;
	} = $props();

	let open = $state(false);
	let eligibleTasks = $state.raw<EligibleTaskChoice[]>([]);
	let selectedTaskId = $state('');
	let linkBusy = $state(false);
	let linkError = $state<string | undefined>();
	let moveConfirmTask = $state<EligibleTaskChoice | null>(null);

	export async function openDialog() {
		linkError = undefined;
		moveConfirmTask = null;
		selectedTaskId = '';
		open = true;
		try {
			eligibleTasks = await featuresApi.eligibleTasks(organizationId, projectId, feature.id);
			selectedTaskId = eligibleTasks[0]?.id ?? '';
		} catch (e) {
			linkError = toAppError(e).message;
		}
	}

	async function submitLinkTask(confirmedMove = false) {
		if (!selectedTaskId || linkBusy) return;
		const chosen = eligibleTasks.find((t) => t.id === selectedTaskId);
		if (!chosen) return;
		if (chosen.currentFeature && !confirmedMove) {
			moveConfirmTask = chosen;
			return;
		}
		linkBusy = true;
		linkError = undefined;
		try {
			await featuresApi.linkTask(organizationId, projectId, feature.id, {
				taskId: chosen.id,
				expectedRevision: feature.revision
			});
			open = false;
			moveConfirmTask = null;
			toast.success(`Linked ${chosen.ref} to ${featureRef(feature)}.`);
			await onlinked();
		} catch (e) {
			linkError = toAppError(e).message;
		} finally {
			linkBusy = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<Dialog.Header>
			<Dialog.Title
				text={moveConfirmTask
					? `Move ${moveConfirmTask.ref} to ${featureRef(feature)}?`
					: `Link a task to ${featureRef(feature)}`}
			/>
			<Dialog.Description>
				{#if moveConfirmTask && moveConfirmTask.currentFeature}
					{moveConfirmTask.ref} is currently linked to {moveConfirmTask.currentFeature.ref}
					{moveConfirmTask.currentFeature.title}. Moving it to {featureRef(feature)}
					{feature.title} keeps its identity and history and changes neither feature's status.
				{:else}
					Select an eligible same-project task to link to {featureRef(feature)} {feature.title}.
				{/if}
			</Dialog.Description>
		</Dialog.Header>

		{#if moveConfirmTask}
			{#if linkError}
				<p class={errorClass} role="alert">{linkError}</p>
			{/if}
			<Dialog.Footer>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={linkBusy}
					onclick={() => (moveConfirmTask = null)}
				>
					Back
				</Button>
				<Button
					type="button"
					class={mobileButton}
					disabled={linkBusy}
					onclick={() => void submitLinkTask(true)}
				>
					{linkBusy ? 'Moving…' : `Move to ${featureRef(feature)}`}
				</Button>
			</Dialog.Footer>
		{:else}
			<div class="flex flex-col gap-4">
				{#if eligibleTasks.length === 0}
					<p class="text-sm/6 text-fg-muted">
						No other eligible tasks in {projectName} right now.
					</p>
				{:else}
					<div>
						<Label for="{uid}-link-select" class="mb-1 block text-sm/6 font-medium text-fg">
							Task
						</Label>
						<select id="{uid}-link-select" bind:value={selectedTaskId} class={selectClass}>
							{#each eligibleTasks as choice (choice.id)}
								<option value={choice.id}>
									{choice.ref}
									{choice.title}{choice.currentFeature
										? ` (currently ${choice.currentFeature.ref})`
										: ''}
								</option>
							{/each}
						</select>
					</div>
				{/if}
				{#if linkError}
					<p class={errorClass} role="alert">{linkError}</p>
				{/if}
				<Dialog.Footer>
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						disabled={linkBusy}
						onclick={() => (open = false)}
					>
						Cancel
					</Button>
					{#if eligibleTasks.length > 0}
						<Button
							type="button"
							class={mobileButton}
							disabled={linkBusy || !selectedTaskId}
							onclick={() => void submitLinkTask(false)}
						>
							{linkBusy ? 'Linking…' : 'Link task'}
						</Button>
					{/if}
				</Dialog.Footer>
			</div>
		{/if}
	</Dialog.Content>
</Dialog.Root>
