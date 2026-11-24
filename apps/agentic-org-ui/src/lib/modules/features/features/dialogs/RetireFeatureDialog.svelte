<!--
  @component
  Retire feature dialog (features-ux §2, FTR-8, FTR-12): requires retired_reason, validates an
  optional same-project replacement with cycle detection, and explicitly confirms that linked open
  tasks stay linked and are never closed silently.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import {
		dangerButton,
		errorClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { toAppError } from '$lib/proto';
	import { featuresApi } from '../../api';
	import { featureRef, validateRetirement } from '../../present';
	import type { FeatureDetail, FeatureRow } from '../../types';

	let {
		open = $bindable(false),
		organizationId,
		projectId,
		feature,
		allFeatures,
		onretired
	}: {
		open: boolean;
		organizationId: string;
		projectId: string;
		feature: FeatureDetail;
		allFeatures: FeatureRow[];
		onretired: () => void;
	} = $props();

	const uid = $props.id();

	let reason = $derived(open ? '' : '');
	let replacementId = $derived(open ? '' : '');
	let busy = $state(false);
	let error = $derived<string | undefined>(open ? undefined : undefined);

	const candidates = $derived(
		allFeatures.filter((f) => f.id !== feature.id && f.status !== 'retired')
	);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		const validation = validateRetirement(reason, replacementId || null, feature.id, allFeatures);
		if (validation) {
			error = validation;
			return;
		}

		busy = true;
		error = undefined;
		try {
			await featuresApi.retire(organizationId, projectId, feature.id, {
				reason: reason.trim(),
				replacedByFeatureId: replacementId || null,
				expectedRevision: feature.revision
			});
			open = false;
			toast.success(`Retired ${featureRef(feature)} ${feature.title}.`);
			onretired();
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<Dialog.Header>
			<Dialog.Title text="Retire {featureRef(feature)} {feature.title}?" />
			<Dialog.Description>
				Records retirement in the canonical feature file. Linked tasks and history are preserved;
				retiring never closes or unlinks open tasks.
			</Dialog.Description>
		</Dialog.Header>

		<form class="flex flex-col gap-4" onsubmit={submit}>
			<div>
				<Label for="{uid}-reason" class="mb-1 block text-sm/6 font-medium text-fg">
					Reason for retirement
				</Label>
				<textarea
					id="{uid}-reason"
					rows={3}
					placeholder="Why is this capability being retired?"
					bind:value={reason}
					aria-invalid={error ? 'true' : undefined}
					class={textareaClass}></textarea>
			</div>

			<div>
				<Label for="{uid}-replacement" class="mb-1 block text-sm/6 font-medium text-fg">
					Replaced by (optional, same project)
				</Label>
				<select id="{uid}-replacement" bind:value={replacementId} class={selectClass}>
					<option value="">No replacement</option>
					{#each candidates as candidate (candidate.id)}
						<option value={candidate.id}>{featureRef(candidate)} {candidate.title}</option>
					{/each}
				</select>
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
				<Button type="submit" class="{mobileButton} {dangerButton}" disabled={busy}>
					{busy ? 'Retiring…' : 'Retire feature'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
