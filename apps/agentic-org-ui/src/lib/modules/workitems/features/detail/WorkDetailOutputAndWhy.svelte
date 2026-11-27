<!--
  @component
  Output & Why sections on /work/[ref] (work-items-ux §9.3; decisions-and-tech-debt §5).
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import { mobileButton, selectClass } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { when } from '../../present';
	import type { TaskDetail, WhyLink } from '../../types';

	let {
		organizationId,
		task,
		stale = false,
		onchanged
	}: {
		organizationId: string;
		task: TaskDetail;
		stale?: boolean;
		onchanged: () => Promise<void> | void;
	} = $props();

	const uid = $props.id();

	let whyOptions = $state.raw<WhyLink[]>([]);
	let whyPickerOpen = $state(false);
	let selectedWhyId = $state('');
	let linkingWhy = $state(false);

	async function openWhyPicker() {
		whyPickerOpen = true;
		try {
			whyOptions = await workItemsApi.whyChoices(organizationId, task.id);
		} catch (e) {
			toast.error(toAppError(e).message);
			whyPickerOpen = false;
		}
	}

	async function submitLinkWhy() {
		if (!selectedWhyId || linkingWhy) return;
		linkingWhy = true;
		try {
			await workItemsApi.linkWhy(organizationId, task.id, selectedWhyId);
			selectedWhyId = '';
			whyPickerOpen = false;
			toast.success('Linked document.');
			await onchanged();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			linkingWhy = false;
		}
	}
</script>

<!-- Output (§9.3) -->
<section
	aria-labelledby="{uid}-output-heading"
	class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="output-section"
>
	<h3 id="{uid}-output-heading" class="text-sm/6 font-semibold text-fg">Output</h3>
	{#if task.completion}
		<div
			class="rounded-md bg-surface p-3 text-sm/6 outline-1 -outline-offset-1 outline-line-subtle"
		>
			<p class="text-xs/5 text-fg-muted">
				Completion note by {task.completion.by.name} · {when(task.completion.at)}
			</p>
			<p class="mt-1 whitespace-pre-wrap text-fg-body">{task.completion.text}</p>
		</div>
	{:else}
		<p class="text-sm/6 text-fg-muted">No outcome recorded.</p>
	{/if}

	{#if task.output.length > 0}
		<ul role="list" class="divide-y divide-line-subtle text-sm/6">
			{#each task.output as out (out.id)}
				<li class="flex flex-wrap items-baseline justify-between gap-2 py-2">
					<div class="flex min-w-0 items-center gap-2">
						<span class="rounded bg-surface-overlay px-1.5 py-0.5 text-xs/5 text-fg-muted">
							{out.kind}
						</span>
						{#if out.href}
							<a
								href={resolve(out.href as '/')}
								class="font-medium text-link hover:underline [overflow-wrap:anywhere]"
							>
								{out.label}
							</a>
						{:else}
							<span class="font-mono text-xs/5 text-fg-secondary [overflow-wrap:anywhere]">
								{out.label}
							</span>
						{/if}
					</div>
					{#if out.unresolved}
						<span class="text-xs/5 text-fg-muted">{out.unresolved}</span>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</section>

<!-- Why block (decisions-and-tech-debt §5) -->
<section
	aria-labelledby="{uid}-why-heading"
	class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="why-section"
>
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h3 id="{uid}-why-heading" class="text-sm/6 font-semibold text-fg">Why</h3>
		{#if task.can.edit && !stale && !whyPickerOpen}
			<Button type="button" variant="outline" size="sm" class={mobileButton} onclick={openWhyPicker}
				>Link document…</Button
			>
		{/if}
	</div>

	{#if task.why.length === 0}
		<p class="text-sm/6 text-fg-muted">No linked decisions, designs or specs yet.</p>
	{:else}
		<ul role="list" class="flex flex-col gap-2 text-sm/6">
			{#each task.why as doc (doc.id)}
				<li class="flex flex-wrap items-center justify-between gap-2">
					<span class="text-fg">
						{#if doc.key}
							<span class="mr-1.5 font-mono text-xs/5 text-fg-muted">{doc.key}</span>
						{/if}
						{doc.title}
					</span>
					<span class="text-xs/5 text-fg-muted">{doc.kind} · {doc.status}</span>
				</li>
			{/each}
		</ul>
	{/if}

	{#if whyPickerOpen}
		<div class="flex flex-wrap items-center gap-2 pt-1">
			<select
				aria-label="Choose document to link"
				bind:value={selectedWhyId}
				disabled={linkingWhy}
				class="min-w-48 flex-1 {selectClass}"
			>
				<option value="">Choose a document…</option>
				{#each whyOptions as opt (opt.id)}
					<option value={opt.id}>
						{opt.key ? `${opt.key} · ` : ''}{opt.title} ({opt.kind})
					</option>
				{/each}
			</select>
			<Button
				type="button"
				size="sm"
				class={mobileButton}
				disabled={!selectedWhyId || linkingWhy}
				onclick={submitLinkWhy}
			>
				{linkingWhy ? 'Linking…' : 'Link'}
			</Button>
			<Button
				type="button"
				variant="outline"
				size="sm"
				class={mobileButton}
				onclick={() => (whyPickerOpen = false)}>Cancel</Button
			>
		</div>
	{/if}
</section>
