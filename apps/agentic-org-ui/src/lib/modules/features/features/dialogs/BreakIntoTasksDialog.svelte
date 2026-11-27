<!--
  @component
  Break into tasks dialog (features-ux §2): proposes editable draft tasks linked to this project and
  feature. Each created task inherits the project's required accountable team and allows independent
  optional assigned team and individual assignee.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import { errorClass, inputClass, mobileButton, selectClass } from '$lib/components/classes';
	import { toAppError } from '$lib/proto';
	import { featuresApi } from '../../api';
	import { featureRef } from '../../present';
	import type { BreakIntoTaskDraft, FeatureDetail, FeatureEmployeeRef } from '../../types';

	let {
		open = $bindable(false),
		organizationId,
		projectId,
		ownerTeamName,
		feature,
		owners,
		teams,
		oncreated
	}: {
		open: boolean;
		organizationId: string;
		projectId: string;
		ownerTeamName: string;
		feature: FeatureDetail;
		owners: FeatureEmployeeRef[];
		teams: { id: string; name: string }[];
		oncreated: () => void;
	} = $props();

	const uid = $props.id();

	let busy = $state(false);
	let error = $state<string | undefined>();
	let extraSeq = 0;

	function seedDrafts(target: FeatureDetail): BreakIntoTaskDraft[] {
		return [
			{
				id: 'draft-seed-1',
				title: `Implement core behaviour for ${target.title}`,
				description: '',
				assignedTeamId: null,
				assigneeId: null,
				priority: 'normal'
			},
			{
				id: 'draft-seed-2',
				title: `Verify acceptance criteria for ${featureRef(target)}`,
				description: '',
				assignedTeamId: null,
				assigneeId: null,
				priority: 'normal'
			}
		];
	}

	let drafts = $state<BreakIntoTaskDraft[]>(untrack(() => seedDrafts(feature)));

	function addDraft() {
		drafts.push({
			id: `draft-extra-${++extraSeq}`,
			title: '',
			description: '',
			assignedTeamId: null,
			assigneeId: null,
			priority: 'normal'
		});
	}

	function removeDraft(id: string) {
		drafts = drafts.filter((d) => d.id !== id);
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		const valid = drafts
			.map((d) => ({ ...d, title: d.title.trim(), description: d.description.trim() }))
			.filter((d) => d.title.length > 0);
		if (valid.length === 0) {
			error = 'Enter a title for at least one task before creating.';
			return;
		}

		busy = true;
		error = undefined;
		try {
			const res = await featuresApi.breakIntoTasks(organizationId, projectId, feature.id, valid);
			open = false;
			toast.success(
				`Created ${res.created.length} ${res.created.length === 1 ? 'task' : 'tasks'} for ${featureRef(feature)}.`
			);
			oncreated();
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title text="Break {featureRef(feature)} into tasks" />
			<Dialog.Description>
				Review and edit draft tasks before creating them. Every created task uses the project's
				accountable team ({ownerTeamName}) and links to {featureRef(feature)}.
			</Dialog.Description>
		</Dialog.Header>

		<form class="flex flex-col gap-4" onsubmit={submit}>
			<div class="flex flex-col gap-3">
				{#each drafts as draft, idx (draft.id)}
					<div
						class="flex flex-col gap-3 rounded-lg bg-surface-raised p-3 ring-1 ring-line"
						data-testid="break-task-draft"
					>
						<div class="flex items-start justify-between gap-2">
							<div class="min-w-0 flex-1">
								<Label
									for="{uid}-draft-title-{idx}"
									class="mb-1 block text-xs/5 font-medium text-fg-secondary"
								>
									Task {idx + 1} title
								</Label>
								<Input
									id="{uid}-draft-title-{idx}"
									bind:value={draft.title}
									placeholder="Describe the task outcome"
									class={inputClass}
								/>
							</div>
							{#if drafts.length > 1}
								<ModalTooltip label="Remove draft task {idx + 1}" side="left" class="mt-6">
									<Button
										type="button"
										variant="outline"
										size="sm"
										class={mobileButton}
										aria-label="Remove draft task {idx + 1}"
										onclick={() => removeDraft(draft.id)}
									>
										<Trash2Icon class="size-4" aria-hidden="true" />
									</Button>
								</ModalTooltip>
							{/if}
						</div>

						<div class="grid grid-cols-1 gap-2 min-[640px]:grid-cols-3">
							<div>
								<Label
									for="{uid}-draft-team-{idx}"
									class="mb-1 block text-xs/5 font-medium text-fg-muted"
								>
									Assigned team
								</Label>
								<select
									id="{uid}-draft-team-{idx}"
									value={draft.assignedTeamId ?? ''}
									onchange={(e) =>
										(draft.assignedTeamId = (e.currentTarget as HTMLSelectElement).value || null)}
									class={selectClass}
								>
									<option value="">Unassigned team</option>
									{#each teams as t (t.id)}
										<option value={t.id}>{t.name}</option>
									{/each}
								</select>
							</div>
							<div>
								<Label
									for="{uid}-draft-assignee-{idx}"
									class="mb-1 block text-xs/5 font-medium text-fg-muted"
								>
									Assignee
								</Label>
								<select
									id="{uid}-draft-assignee-{idx}"
									value={draft.assigneeId ?? ''}
									onchange={(e) =>
										(draft.assigneeId = (e.currentTarget as HTMLSelectElement).value || null)}
									class={selectClass}
								>
									<option value="">Unassigned</option>
									{#each owners as emp (emp.id)}
										<option value={emp.id}>{emp.name} ({emp.kind})</option>
									{/each}
								</select>
							</div>
							<div>
								<Label
									for="{uid}-draft-prio-{idx}"
									class="mb-1 block text-xs/5 font-medium text-fg-muted"
								>
									Priority
								</Label>
								<select id="{uid}-draft-prio-{idx}" bind:value={draft.priority} class={selectClass}>
									<option value="urgent">Urgent</option>
									<option value="high">High</option>
									<option value="normal">Normal</option>
									<option value="low">Low</option>
								</select>
							</div>
						</div>
					</div>
				{/each}
			</div>

			<div>
				<Button type="button" variant="outline" size="sm" class={mobileButton} onclick={addDraft}>
					<PlusIcon data-icon="inline-start" />
					Add another task
				</Button>
			</div>

			{#if error}
				<p class={errorClass} role="alert">{error}</p>
			{/if}

			<Dialog.Footer>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => (open = false)}
				>
					Cancel
				</Button>
				<Button type="submit" class={mobileButton} disabled={busy}>
					{busy
						? 'Creating tasks…'
						: `Create ${drafts.length} ${drafts.length === 1 ? 'task' : 'tasks'}`}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
