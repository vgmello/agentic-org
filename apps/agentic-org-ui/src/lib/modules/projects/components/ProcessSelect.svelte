<!--
  @component
  The Development process select (development-processes §3, PKG-4): every current development process of the
  catalogue and "None"; drafts and deprecated ones can't be chosen. When the choice differs from `current`, it says
  what choosing it does before anything is saved: the roles it creates, the existing roles it uses and the sections
  the project gains. Nobody is hired, started or told anything by the choice itself.
-->
<script lang="ts">
	import { selectClass, helpClass, errorClass } from '$lib/components/classes';
	import { Label } from '$lib/components/ui/label';
	import { Resource } from '$lib/components/resource.svelte';
	import { processesApi } from '$lib/modules/processes';
	import { toAppError } from '$lib/proto';
	import { projectsApi } from '../api';
	import type { ProcessPreview } from '../types';

	let {
		id,
		organizationId,
		projectId,
		value = $bindable(''),
		current = '',
		disabled = false,
		error
	}: {
		id: string;
		organizationId: string;
		/** null while the project is being created. */
		projectId: string | null;
		/** The chosen process ID; '' is None. */
		value: string;
		/** What the project follows now ('' for none): no preview while the choice is unchanged. */
		current?: string;
		disabled?: boolean;
		/** A refusal of this field, shown on it. */
		error?: string;
	} = $props();

	const catalogue = new Resource(() => processesApi.devProcesses(organizationId));
	const choices = $derived(
		(catalogue.data?.processes ?? [])
			.filter((p) => p.status === 'current' && !p.invalid)
			.toSorted((a, b) => a.name.localeCompare(b.name))
	);

	let preview = $state<{ for: string; value?: ProcessPreview; error?: string }>();
	let previewRun = 0;

	async function load(processId: string) {
		const run = ++previewRun;
		preview = { for: processId };
		try {
			const answer = await projectsApi.processPreview(organizationId, projectId, processId);
			if (run === previewRun) preview = { for: processId, value: answer };
		} catch (e) {
			if (run === previewRun)
				preview = {
					for: processId,
					error: toAppError(e).message
				};
		}
	}

	function changed() {
		if (value && value !== current) void load(value);
		else preview = undefined;
	}

	const names = (list: { name: string }[]) => list.map((r) => r.name).join(', ');
	const describedBy = $derived(
		[error ? `${id}-error` : `${id}-help`, preview ? `${id}-preview` : ''].filter(Boolean).join(' ')
	);
</script>

<div>
	<Label for={id}>Development process</Label>
	<select
		{id}
		bind:value
		onchange={changed}
		disabled={disabled || catalogue.state === 'loading'}
		aria-invalid={error ? true : undefined}
		aria-describedby={describedBy}
		class="mt-2 {selectClass}"
	>
		<option value="">None</option>
		{#each choices as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
		{#if current && !choices.some((p) => p.id === current)}
			<!-- What the project follows now stays shown even when it can no longer be chosen. -->
			<option value={current} disabled
				>The current process (no longer current in the catalogue)</option
			>
		{/if}
	</select>
	{#if error}
		<p id="{id}-error" class={errorClass}>{error}</p>
	{:else if catalogue.state === 'error' || catalogue.state === 'unavailable'}
		<p id="{id}-help" class={helpClass}>
			Couldn't load the development processes, so only None can be chosen now.
			{#if catalogue.state === 'error'}{catalogue.message}{/if}
		</p>
	{:else}
		<p id="{id}-help" class={helpClass}>
			How the project builds software: its roles, skills and tabs. None keeps the fixed tabs. It can
			be changed later.
		</p>
	{/if}
	{#if preview}
		<div
			id="{id}-preview"
			class="mt-3 rounded-md border border-line p-3 text-xs/5 text-fg-secondary"
			aria-live="polite"
			data-testid="process-preview"
		>
			{#if preview.error}
				<p class="text-danger-text">Couldn't check what it would add: {preview.error}</p>
			{:else if !preview.value}
				<p>Checking what it would add…</p>
			{:else}
				{@const v = preview.value}
				<p class="font-semibold text-fg">Choosing it will:</p>
				<ul role="list" class="mt-1 list-disc space-y-1 pl-5 marker:text-fg-muted">
					<li>
						{v.createsRoles.length
							? `Create the roles ${names(v.createsRoles)}`
							: 'Create no roles: the organization has them all'}
					</li>
					{#if v.usesRoles.length}<li>Use the existing roles {names(v.usesRoles)}</li>{/if}
					{#if v.addsSections.length}<li>
							Add the documentation sections {v.addsSections.join(', ')}
						</li>{/if}
				</ul>
				<p class="mt-2">Nobody is hired or started by choosing it.</p>
			{/if}
		</div>
	{/if}
</div>
