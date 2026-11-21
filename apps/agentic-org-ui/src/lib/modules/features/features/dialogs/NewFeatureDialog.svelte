<!--
  @component
  New feature dialog (features-ux §2, FTR-13): validates title and local slug, creates a Planned
  feature record in organization Git (no code repository required) and navigates to its editor.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { errorClass, inputClass, mobileButton, selectClass } from '$lib/components/classes';
	import { toAppError } from '$lib/proto';
	import { featuresApi } from '../../api';
	import { FEATURE_TEMPLATE, checkFeatureSlug, featureHref, slugifyFeature } from '../../present';
	import type { FeatureEmployeeRef } from '../../types';

	let {
		open = $bindable(false),
		organizationId,
		projectId,
		projectSlug,
		owners,
		oncreated
	}: {
		open: boolean;
		organizationId: string;
		projectId: string;
		projectSlug: string;
		owners: FeatureEmployeeRef[];
		oncreated?: () => void;
	} = $props();

	const uid = $props.id();

	let title = $derived(open ? '' : '');
	let localSlug = $derived(open ? '' : '');
	let slugTouched = $derived(open ? false : false);
	let ownerId = $derived(open ? '' : '');
	let busy = $state(false);
	let titleError = $derived<string | undefined>(open ? undefined : undefined);
	let slugError = $derived<string | undefined>(open ? undefined : undefined);
	let submitError = $derived<string | undefined>(open ? undefined : undefined);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		titleError = title.trim() ? undefined : 'Enter a title for the feature.';
		slugError = checkFeatureSlug(localSlug);
		if (titleError || slugError) return;

		busy = true;
		submitError = undefined;
		try {
			const created = await featuresApi.create(organizationId, projectId, {
				title: title.trim(),
				localSlug: localSlug.trim(),
				ownerId: ownerId || null,
				bodyMarkdown: FEATURE_TEMPLATE
			});
			open = false;
			toast.success(`Created feature ${title.trim()}.`);
			oncreated?.();
			await goto(resolve(`${featureHref(projectId, created.id)}?edit=1` as '/'));
		} catch (e) {
			submitError = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<Dialog.Header>
			<Dialog.Title text="New feature" />
			<Dialog.Description>
				Creates a Planned capability definition in organization Git and opens its editor. No code
				repository is required.
			</Dialog.Description>
		</Dialog.Header>

		<form class="flex flex-col gap-4" onsubmit={submit}>
			<div>
				<Label for="{uid}-title" class="mb-1 block text-sm/6 font-medium text-fg">Title</Label>
				<Input
					id="{uid}-title"
					placeholder="Quick capture of requests"
					bind:value={title}
					oninput={() => {
						if (!slugTouched) localSlug = slugifyFeature(title);
					}}
					aria-invalid={titleError ? 'true' : undefined}
					class={inputClass}
				/>
				{#if titleError}
					<p class={errorClass} role="alert">{titleError}</p>
				{/if}
			</div>

			<div>
				<Label for="{uid}-slug" class="mb-1 block text-sm/6 font-medium text-fg">Local slug</Label>
				<Input
					id="{uid}-slug"
					placeholder="quick-capture"
					bind:value={localSlug}
					oninput={() => (slugTouched = true)}
					aria-invalid={slugError ? 'true' : undefined}
					class="font-mono {inputClass}"
				/>
				<p class="mt-1 text-xs/5 text-fg-muted">
					Public slug: <span class="font-mono">{projectSlug}/{localSlug || '…'}</span>
				</p>
				{#if slugError}
					<p class={errorClass} role="alert">{slugError}</p>
				{/if}
			</div>

			<div>
				<Label for="{uid}-owner" class="mb-1 block text-sm/6 font-medium text-fg">
					Accountable owner (optional while Planned)
				</Label>
				<select id="{uid}-owner" bind:value={ownerId} class={selectClass}>
					<option value="">Unassigned</option>
					{#each owners as emp (emp.id)}
						<option value={emp.id}>{emp.name} ({emp.kind})</option>
					{/each}
				</select>
			</div>

			{#if submitError}
				<p class={errorClass} role="alert">{submitError}</p>
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
					{busy ? 'Creating…' : 'Create feature'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
