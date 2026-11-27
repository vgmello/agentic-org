<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import type { DraftTask, Member, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { nameOf } from '$lib/modules/work/people';
	import { requestRef } from '$lib/modules/work/requests-view';

	export type RequestStep = 'route' | 'approve' | 'changes' | 'decline' | 'merge';
	type OrgProject = NonNullable<OrgSnapshot['projects']>[number];

	let {
		step,
		summary,
		snapshot,
		suggestions,
		agents,
		others,
		projects,
		dropped,
		keptCount,
		busy,
		canSubmit,
		field,
		teamStands,
		assigneeStands,
		target = $bindable(''),
		into = $bindable(''),
		projectId = $bindable(''),
		note = $bindable(''),
		drafts = $bindable<{ draft: DraftTask; title: string; keep: boolean }[]>([]),
		onsubmit,
		oncancel
	}: {
		step: RequestStep;
		summary: WorkItemSummary;
		snapshot: OrgSnapshot;
		suggestions: { member: Member; why: string }[];
		agents: Member[];
		others: WorkItemSummary[];
		projects: OrgProject[];
		dropped: number;
		keptCount: number;
		busy: boolean;
		canSubmit: boolean;
		field: string;
		teamStands: (id: string | undefined) => boolean;
		assigneeStands: (id: string | undefined) => boolean;
		target: string;
		into: string;
		projectId: string;
		note: string;
		drafts: { draft: DraftTask; title: string; keep: boolean }[];
		onsubmit: () => void;
		oncancel: () => void;
	} = $props();

	const submitBtnLabel = $derived.by(() => {
		if (step === 'route') return target ? 'Send' : 'Take back';
		if (step === 'merge') return 'Merge';
		if (step === 'approve') return `Create ${keptCount} ${keptCount === 1 ? 'task' : 'tasks'}`;
		if (step === 'changes') return 'Send back';
		return 'Decline';
	});
</script>

<section
	class="flex flex-col gap-2 rounded-lg bg-surface-raised p-3 outline-1 -outline-offset-1 outline-line"
	data-testid="request-step"
>
	{#if step === 'route'}
		<Label for="req-scoper">Who scopes it</Label>
		<select id="req-scoper" bind:value={target} class={field}>
			<option value=""
				>{summary.status === 'new' ? 'Choose an agent…' : 'Nobody (take it back)'}</option
			>
			{#each suggestions as x (x.member.id)}<option value={x.member.id}
					>{x.member.name} · {x.why}</option
				>{/each}
			{#each agents.filter((a) => !suggestions.some((x) => x.member.id === a.id)) as a (a.id)}<option
					value={a.id}>{a.name}{a.agent?.status === 'paused' ? ' (paused)' : ''}</option
				>{/each}
		</select>
		{#if snapshot.members.find((m) => m.id === target)?.agent?.status === 'paused'}<p
				class="t-red rounded-md px-3 py-2 text-sm/6"
				role="alert"
			>
				{nameOf(snapshot, target)} is paused and won't pick this up. Choose someone else or leave it not
				routed.
			</p>{/if}
	{:else if step === 'merge'}
		<Label for="req-into">Merge into</Label>
		<select id="req-into" bind:value={into} class={field}
			><option value="">Choose a request…</option>{#each others as o (o.id)}<option value={o.id}
					>{requestRef(o)} · {o.title}</option
				>{/each}</select
		>
		<p class="text-xs/5 text-fg-muted">
			Its words are added to that request as a note, and you stop seeing this one in the queue.
		</p>
	{:else if step === 'approve'}
		<Label for="req-project">Which project?</Label>
		<select id="req-project" bind:value={projectId} class={field}
			><option value="">Choose a project…</option>{#each projects as p (p.id)}<option value={p.id}
					>{p.name}</option
				>{/each}</select
		>
		{#if projects.length === 0}<p class="t-red rounded-md px-3 py-2 text-sm/6" role="alert">
				There is no project to put the tasks in. Create a project first.
			</p>{/if}
		<p class="text-sm/6 text-fg-body">
			These become tasks in {projectId
				? (projects.find((p) => p.id === projectId)?.name ?? 'that project')
				: 'that project'}. Edit or remove any of them first.
		</p>
		{#if dropped > 0}<p class="text-xs/5 text-fg-muted" data-testid="drafts-dropped">
				{dropped}
				{dropped === 1 ? 'draft names' : 'drafts name'} a team or person that doesn't apply there, so
				{dropped === 1 ? 'it is' : 'they are'} created without it.
			</p>{/if}
		<ul class="flex flex-col gap-2" aria-label="Tasks to create">
			{#each drafts as d, i (i)}
				<li class="flex flex-wrap items-center gap-x-2">
					<input type="checkbox" bind:checked={d.keep} aria-label="Create task {i + 1}" />
					<Input
						bind:value={d.title}
						class="min-w-0 flex-1 basis-40 focus-visible:ring-0 focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-focus"
						disabled={!d.keep}
						aria-label="Task {i + 1} title"
						autocomplete="off"
					/>
					{#if d.keep && d.title.trim() !== '' && !teamStands(d.draft.team_id)}<span
							class="shrink-0 text-xs/5 text-fg-muted"
							data-testid="draft-without-team">loses its team</span
						>{/if}
					{#if d.keep && d.title.trim() !== '' && !assigneeStands(d.draft.assignee_id)}<span
							class="shrink-0 text-xs/5 text-fg-muted"
							data-testid="draft-without-person">loses its assignee</span
						>{/if}
				</li>
			{/each}
		</ul>
	{:else}
		<Label for="req-note">{step === 'changes' ? 'What should change' : 'Why'}</Label>
		<Input
			id="req-note"
			bind:value={note}
			class="focus-visible:ring-0 focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-focus"
			autocomplete="off"
		/>
	{/if}
	<div class="flex flex-wrap gap-2">
		<Button
			size="sm"
			class="h-auto min-h-9 max-w-full whitespace-normal text-wrap max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
			disabled={busy || !canSubmit}
			onclick={onsubmit}
		>
			{submitBtnLabel}
		</Button>
		<Button
			size="sm"
			variant="outline"
			class="h-auto min-h-9 max-w-full whitespace-normal text-wrap max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
			onclick={oncancel}>Cancel</Button
		>
	</div>
</section>
