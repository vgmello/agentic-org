<!--
  @component
  Description / Request text field with @-mention assignee picker for CaptureDialog.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import X from '@lucide/svelte/icons/x';
	import { Label } from '$lib/components/ui/label';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import { textareaClass } from '$lib/components/classes';
	import type { EmployeeChoice, TaskType } from '../../types';
	import {
		applyMentionReplacement,
		detectMentionMatch,
		filterMentionCandidates,
		type MentionMatch
	} from './composer-helpers';

	let {
		uid,
		type,
		employees,
		disabled,
		description = $bindable(''),
		mentionedIds = $bindable<string[]>([]),
		assigneeId = $bindable('')
	}: {
		uid: string;
		type: TaskType;
		employees: readonly EmployeeChoice[];
		disabled: boolean;
		description: string;
		mentionedIds: string[];
		assigneeId: string;
	} = $props();

	let mentionMatch = $state<MentionMatch | null>(null);
	let mentionIdx = $state(0);
	let descriptionInput = $state<HTMLTextAreaElement | null>(null);

	export function resetMentions() {
		mentionMatch = null;
		mentionIdx = 0;
	}

	const mentionCandidates = $derived(
		mentionMatch ? filterMentionCandidates(employees, mentionMatch.query) : []
	);
	const mentionedEmployees = $derived(
		mentionedIds
			.map((id) => employees.find((e) => e.id === id))
			.filter((e): e is EmployeeChoice => Boolean(e))
	);

	function handleInput(event: Event) {
		const el = event.currentTarget as HTMLTextAreaElement;
		description = el.value;
		mentionMatch = detectMentionMatch(el.value, el.selectionStart ?? el.value.length);
		mentionIdx = 0;
	}

	function pickMention(emp: EmployeeChoice) {
		if (!mentionMatch) return;
		const applied = applyMentionReplacement(description, mentionMatch, emp);
		description = applied.text;
		if (!mentionedIds.includes(emp.id)) mentionedIds = [...mentionedIds, emp.id];
		if (!assigneeId) assigneeId = emp.id;
		mentionMatch = null;
		mentionIdx = 0;
		void tick().then(() => {
			if (!descriptionInput) return;
			descriptionInput.focus();
			descriptionInput.setSelectionRange(applied.caret, applied.caret);
		});
	}

	function removeMention(id: string) {
		const next = mentionedIds.filter((item) => item !== id);
		mentionedIds = next;
		if (assigneeId === id) assigneeId = next[0] ?? '';
	}

	function handleKeydown(event: KeyboardEvent) {
		if (!mentionMatch || mentionCandidates.length === 0) return;
		if (event.key === 'ArrowDown') {
			event.preventDefault();
			mentionIdx = (mentionIdx + 1) % mentionCandidates.length;
		} else if (event.key === 'ArrowUp') {
			event.preventDefault();
			mentionIdx = (mentionIdx - 1 + mentionCandidates.length) % mentionCandidates.length;
		} else if (event.key === 'Enter' || event.key === 'Tab') {
			event.preventDefault();
			pickMention(mentionCandidates[Math.min(mentionIdx, mentionCandidates.length - 1)]);
		} else if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			mentionMatch = null;
		}
	}
</script>

<div class="relative">
	<Label for="{uid}-desc" class="block text-sm/6 font-medium text-fg">
		{type === 'request' ? 'Details and why it matters' : 'Description (optional)'}
	</Label>
	<textarea
		id="{uid}-desc"
		bind:this={descriptionInput}
		rows={4}
		value={description}
		oninput={handleInput}
		onkeydown={handleKeydown}
		{disabled}
		placeholder={type === 'request'
			? 'Describe the outcome you want… Type @ to tag an agent or person'
			: 'Context, acceptance notes, or links… Type @ to tag an agent or person'}
		class="mt-1.5 {textareaClass}"></textarea>

	{#if mentionMatch && mentionCandidates.length > 0}
		<div
			role="listbox"
			aria-label="Tag agent or person"
			class="work-mention-popover absolute right-0 left-0 z-30 mt-1 flex flex-col gap-0.5 p-1"
			data-testid="composer-mention-popover"
		>
			{#each mentionCandidates as emp, idx (emp.id)}
				<button
					type="button"
					role="option"
					aria-selected={idx === mentionIdx}
					onmousedown={(e) => {
						e.preventDefault();
						pickMention(emp);
					}}
					onmouseenter={() => (mentionIdx = idx)}
					class="work-mention-opt flex w-full items-center justify-between gap-2 rounded-md px-2.5 py-1.5 text-left text-xs/5 {idx ===
					mentionIdx
						? 'is-active'
						: ''}"
				>
					<span class="flex min-w-0 items-center gap-2">
						<span class="shrink-0 font-medium text-fg">{emp.name}</span>
						{#if emp.role}
							<span class="truncate text-fg-muted">{emp.role}</span>
						{/if}
					</span>
					<span class="shrink-0 text-[11px]/4 text-fg-muted">
						{emp.kind} · {emp.availability}
					</span>
				</button>
			{/each}
		</div>
	{/if}

	{#if mentionedEmployees.length > 0}
		<div class="mt-2 flex flex-wrap gap-1.5" aria-label="Tagged members">
			{#each mentionedEmployees as emp (emp.id)}
				<span
					class="inline-flex items-center gap-1 rounded-full bg-surface-raised px-2.5 py-0.5 text-xs/5 text-fg ring-1 ring-line"
				>
					<span>@{emp.name.split(' ')[0]}</span>
					{#if emp.role}
						<span class="text-fg-muted">{emp.role}</span>
					{/if}
					<ModalTooltip label="Remove {emp.name}" side="bottom-start">
						<button
							type="button"
							aria-label="Remove {emp.name}"
							onclick={() => removeMention(emp.id)}
							class="ml-0.5 rounded-full p-0.5 text-fg-muted hover:text-fg"
						>
							<X class="size-3" aria-hidden="true" />
						</button>
					</ModalTooltip>
				</span>
			{/each}
		</div>
	{/if}
</div>

<style>
	.work-mention-popover {
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--popover, var(--surface-overlay));
		-webkit-backdrop-filter: blur(18px);
		backdrop-filter: blur(18px);
		box-shadow: var(--exec-cmdk-menu-shadow, 0 12px 24px -8px var(--surface-sunken));
	}
	.work-mention-opt {
		cursor: pointer;
	}
	.work-mention-opt:hover,
	.work-mention-opt.is-active {
		background: var(--exec-cmdk-opt-active-bg, var(--surface-raised));
	}
</style>
