<script lang="ts">
	import { untrack } from 'svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { newId } from '$lib/org/commands';
	import { isFormer, teamsOfProject } from '$lib/org/model';
	import type { DraftTask, OrgSnapshot, WorkEvent, WorkItem, WorkItemSummary } from '$lib/proto';
	import { workApi, type WorkDraft } from '$lib/work';
	import { shortDate } from '$lib/work/format';
	import { nameOf } from '$lib/work/people';
	import { activityLines, estimateWords, kindWord, OPEN_STATUSES, requestRef, stateWords, suggestScopers } from '$lib/work/requests-view';
	import { STATUS_LABEL, taskRef } from '$lib/work/tasks';

	/** A request's page (feature-requests-ux §3): the words as typed, the conversation, the outcome once scoped, and the owner's next moves. */
	let {
		open = $bindable(false),
		organizationId,
		requestId,
		start,
		snapshot,
		items,
		send,
		onchanged
	}: {
		open: boolean;
		organizationId: string;
		requestId: string | undefined;
		/** A step to begin as soon as the page is there. */
		start?: Step;
		snapshot: OrgSnapshot;
		/** Every work item (the tasks an approval made, and the other open requests, are looked up here). */
		items: WorkItemSummary[];
		send: (draft: WorkDraft) => Promise<unknown>;
		onchanged: (message?: string) => void;
	} = $props();

	type Step = 'route' | 'approve' | 'changes' | 'decline' | 'merge';
	let item = $state<WorkItem | undefined>();
	let events = $state<WorkEvent[]>([]);
	let error = $state<string | undefined>();
	let busy = $state(false);
	let step = $state<Step | undefined>();
	let note = $state('');
	let target = $state('');
	let into = $state('');
	let projectId = $state('');
	// The drafts as the owner edits them before they become tasks (RQS-9): the ones kept, with their words.
	let drafts = $state<{ draft: DraftTask; title: string; keep: boolean }[]>([]);

	// Each load takes a number: an answer that arrives after a newer load began (another request was opened) is dropped.
	let loads = 0;
	async function load() {
		if (!requestId) return;
		const mine = ++loads;
		try {
			const [got, history] = await Promise.all([workApi.get(organizationId, requestId), workApi.activity(organizationId, requestId)]);
			if (mine !== loads) return;
			[item, events] = [got, history];
		} catch (e) {
			if (mine === loads) error = toAppError(e).message;
		}
	}

	$effect(() => {
		if (open && requestId) {
			untrack(() => {
				item = undefined;
				events = [];
				error = undefined;
				step = undefined;
				note = target = into = projectId = '';
				drafts = [];
				busy = false;
				void load().then(() => start && item && OPEN_STATUSES.includes(item.summary.status) && begin(start));
			});
		}
	});

	async function act(draft: WorkDraft, message?: string): Promise<boolean> {
		if (busy) return false;
		busy = true;
		error = undefined;
		const forRequest = requestId;
		try {
			await send(draft);
			// The sheet may have moved on to another request while this was sent: only tell about it.
			if (forRequest === requestId) await load();
			onchanged(message);
			return forRequest === requestId;
		} catch (e) {
			const err = toAppError(e);
			error = err.message;
			if (err.source === 'api' && err.code === 'conflict') await load();
			return false;
		} finally {
			busy = false;
		}
	}

	const s = $derived(item?.summary);
	const v = $derived(s?.version ?? 0);
	const me = $derived(snapshot.members.find((m) => m.owner)?.id ?? 'owner');
	const open_ = $derived(!!s && OPEN_STATUSES.includes(s.status));
	const projects = $derived((snapshot.projects ?? []).filter((p) => !p.archived));
	const projectName = $derived(s?.project_id ? ((snapshot.projects ?? []).find((p) => p.id === s.project_id)?.name ?? 'Unknown project') : 'No project yet');
	const suggestions = $derived(s ? suggestScopers(snapshot, (s.request_kind === 'unknown' ? undefined : s.request_kind) ?? 'feature', s.project_id) : []);
	const agents = $derived(snapshot.members.filter((m) => m.kind === 'agent' && !isFormer(m)));
	const lines = $derived(activityLines(events, snapshot, me));
	const outcome = $derived(item?.outcome);
	const estimate = $derived(estimateWords(outcome));
	const others = $derived(items.filter((i) => i.kind === 'request' && !i.deleted && i.id !== s?.id && OPEN_STATUSES.includes(i.status)));
	// The tasks an approval made, and how far they are.
	const made = $derived((item?.backlinks ?? []).filter((l) => l.kind === 'from_request').map((l) => items.find((i) => i.id === l.from_item_id)).filter((t): t is WorkItemSummary => !!t && !t.deleted));
	const mergedInto = $derived(item?.merged_into ? items.find((i) => i.id === item?.merged_into) : undefined);
	const field = 'w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400';
	const kept = $derived(drafts.filter((d) => d.keep && d.title.trim() !== ''));
	// A draft's team must work on the project chosen here, and its assignee must still be in the organization; otherwise the task is made without it.
	const projectTeams = $derived(projectId ? teamsOfProject(snapshot, projectId).map((t) => t.id) : []);
	const teamStands = (id: string | undefined) => !id || projectTeams.includes(id);
	const assigneeStands = (id: string | undefined) => !id || snapshot.members.some((m) => m.id === id && !isFormer(m));
	const dropped = $derived(kept.filter((d) => !teamStands(d.draft.team_id) || !assigneeStands(d.draft.assignee_id)).length);
	const close = () => ((step = undefined), (note = ''), (target = ''), (into = ''));

	function begin(next: Step) {
		step = next;
		error = undefined;
		note = target = into = '';
		if (next === 'approve') {
			projectId = (s?.project_id && projects.some((p) => p.id === s.project_id) ? s.project_id : '') ?? '';
			drafts = (outcome?.draft_tasks ?? []).map((draft) => ({ draft, title: draft.title, keep: true }));
		}
	}

	async function submitStep() {
		if (!s) return;
		let ok = false;
		if (step === 'route') {
			const pick = suggestions.find((x) => x.member.id === target);
			ok = await act({ method: 'request.route', item_id: s.id, expected_version: v, scoper_id: target || undefined, route_reason: pick?.why, routed_by: target ? 'person' : undefined }, target ? `${requestRef(s)} sent to ${nameOf(snapshot, target)}.` : `${requestRef(s)} taken back.`);
		} else if (step === 'changes') ok = await act({ method: 'request.ask_changes', item_id: s.id, expected_version: v, comment_id: newId(), note }, `${requestRef(s)} sent back for changes.`);
		else if (step === 'decline') ok = await act({ method: 'request.decline', item_id: s.id, expected_version: v, reason: note }, `${requestRef(s)} declined.`);
		else if (step === 'merge') ok = await act({ method: 'request.merge', item_id: s.id, expected_version: v, into, comment_id: newId() }, `${requestRef(s)} merged.`);
		else if (step === 'approve') {
			// The project picked here (a request with none, or one the owner changes their mind about) is set first; the tasks need it.
			if (projectId !== (s.project_id ?? '') && !(await act({ method: 'request.update', item_id: s.id, expected_version: v, project_id: projectId }))) return;
			ok = await act(
				{
					method: 'request.approve',
					item_id: s.id,
					expected_version: item?.summary.version ?? v,
					tasks: kept.map((d) => ({ item_id: newId(), link_id: newId(), title: d.title.trim(), description: d.draft.description, assignee_id: assigneeStands(d.draft.assignee_id) ? d.draft.assignee_id : undefined, team_id: teamStands(d.draft.team_id) ? d.draft.team_id : undefined, priority: d.draft.priority }))
				},
				`${requestRef(s)} approved: ${kept.length} ${kept.length === 1 ? 'task' : 'tasks'} created.`
			);
		}
		if (ok) close();
	}
	const canSubmit = $derived(
		step === 'route' ? s?.status !== 'new' || target !== '' : step === 'merge' ? into !== '' : step === 'approve' ? kept.length > 0 && projectId !== '' : note.trim() !== ''
	);
</script>

<Sheet bind:open>
	<SheetContent side="right" class="w-full max-w-xl overflow-y-auto">
		<SheetHeader>
			<SheetTitle class="[overflow-wrap:anywhere]">{s ? `${requestRef(s)} · ${s.title}` : 'Request'}</SheetTitle>
			<SheetDescription>{s ? `Raised ${shortDate(s.created_at)} by ${nameOf(snapshot, s.created_by)}` : 'Loading…'}</SheetDescription>
		</SheetHeader>

		{#if s && item}
			<div class="flex flex-col gap-y-4 px-4 pb-6" data-testid="request-page">
				<div class="flex flex-wrap items-center gap-2 text-sm/6">
					<Badge variant="secondary" data-testid="request-state">{stateWords(s, snapshot)}</Badge>
					<span class="text-gray-400" data-testid="request-meta">{kindWord(s.request_kind)} · {projectName}</span>
					{#if item.route_reason && (s.status === 'routed' || s.status === 'scoping')}<span class="text-xs/5 text-gray-400 [overflow-wrap:anywhere]" data-testid="route-reason">Sent because: {item.route_reason}{item.routed_by === 'rule' ? ' (suggested by rule)' : ''}</span>{/if}
				</div>

				<section class="space-y-1 text-sm/6" aria-labelledby="req-words">
					<h3 id="req-words" class="font-semibold text-white">Your words</h3>
					<p class="whitespace-pre-wrap rounded-md bg-white/5 px-3 py-2 text-gray-200 [overflow-wrap:anywhere]" data-testid="request-words">{item.text}</p>
				</section>

				{#if outcome}
					<section class="space-y-2 text-sm/6" aria-labelledby="req-outcome" data-testid="request-outcome">
						<h3 id="req-outcome" class="font-semibold text-white">Outcome</h3>
						{#if outcome.feature && outcome.feature.type !== 'unknown'}<p class="text-gray-200">{outcome.feature.type === 'new' ? `A new feature: ${s.title}` : 'Adds to an existing feature'}</p>{/if}
						{#if outcome.docs.length > 0}<p class="text-gray-300">Docs: {outcome.docs.map((d) => d.doc_key).join(', ')}</p>{/if}
						{#if estimate}<p class="text-gray-200" data-testid="request-estimate">Estimate: {estimate}</p>{/if}
						{#if made.length > 0}
							<p class="text-gray-300" data-testid="proposed-as-approved">Proposed: {outcome.draft_tasks.length} {outcome.draft_tasks.length === 1 ? 'task' : 'tasks'} (the ones you approved are below).</p>
						{:else}
						<ul class="divide-y divide-white/5 rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Proposed tasks">
							{#each outcome.draft_tasks as d, i (i)}
								<li class="px-3 py-2 text-gray-200 [overflow-wrap:anywhere]" data-testid="draft-task">{d.title}{#if d.assignee_id}<span class="text-gray-400"> · {nameOf(snapshot, d.assignee_id)}</span>{/if}</li>
							{:else}
								<li class="px-3 py-2 text-gray-400">No tasks proposed.</li>
							{/each}
						</ul>
						{/if}
					</section>
				{/if}

				{#if made.length > 0}
					<section class="space-y-1 text-sm/6" aria-labelledby="req-made" data-testid="request-tasks">
						<h3 id="req-made" class="font-semibold text-white">Tasks created · {made.filter((t) => t.status === 'done').length} of {made.length} done</h3>
						<ul class="divide-y divide-white/5 rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10">
							{#each made as t (t.id)}
								<li class="flex items-center gap-x-3 px-3 py-2"><span class="text-gray-400">{taskRef(t)}</span><span class="min-w-0 flex-1 [overflow-wrap:anywhere]">{t.title}</span><span class="text-gray-300">{STATUS_LABEL[t.status as 'todo'] ?? t.status}</span></li>
							{/each}
						</ul>
					</section>
				{/if}

				{#if s.status === 'declined' && item.decline_reason}<p class="rounded-md bg-white/5 px-3 py-2 text-sm/6 text-gray-200 [overflow-wrap:anywhere]" data-testid="decline-reason">Declined: {item.decline_reason}</p>{/if}
				{#if s.status === 'merged'}<p class="rounded-md bg-white/5 px-3 py-2 text-sm/6 text-gray-200" data-testid="merged-into">Merged into {mergedInto ? `${requestRef(mergedInto)} · ${mergedInto.title}` : 'another request'}.</p>{/if}

				<section class="space-y-2 text-sm/6" aria-labelledby="req-talk">
					<h3 id="req-talk" class="font-semibold text-white">What happened</h3>
					<ol class="space-y-2" data-testid="request-history">
						{#each lines as l, i (i)}
							<li class="text-gray-300"><span class="text-gray-400">{shortDate(l.at)}</span> · {l.text}{#if l.note}<p class="mt-1 whitespace-pre-wrap rounded-md bg-white/5 px-3 py-2 text-gray-200 [overflow-wrap:anywhere]">{l.note}</p>{/if}</li>
						{/each}
					</ol>
				</section>

				{#if !step && open_}
					<div class="flex flex-wrap gap-2" data-testid="request-actions">
						{#if s.status === 'scoped'}
							{#if (outcome?.draft_tasks.length ?? 0) > 0}
								<Button size="sm" onclick={() => begin('approve')}>Approve and create {outcome?.draft_tasks.length} {outcome?.draft_tasks.length === 1 ? 'task' : 'tasks'}…</Button>
							{:else}
								<p class="basis-full text-sm/6 text-amber-200" data-testid="no-drafts">No tasks were proposed, so there is nothing to approve. Ask for changes.</p>
							{/if}
							<Button size="sm" variant="outline" onclick={() => begin('changes')}>Ask for changes…</Button>
						{/if}
						{#if s.status !== 'scoped'}<Button size="sm" variant="outline" onclick={() => begin('route')}>{s.status === 'new' ? 'Route…' : 'Change who scopes it…'}</Button>{/if}
						<Button size="sm" variant="outline" onclick={() => begin('merge')} disabled={others.length === 0} aria-describedby={others.length === 0 ? 'req-no-merge' : undefined}>Merge into…</Button>
						<Button size="sm" variant="outline" onclick={() => begin('decline')}>Decline…</Button>
						{#if others.length === 0}<p id="req-no-merge" class="basis-full text-xs/5 text-gray-400" data-testid="no-merge">No other open requests to merge into.</p>{/if}
					</div>
				{:else if step}
					<section class="space-y-2 rounded-lg bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10" data-testid="request-step">
						{#if step === 'route'}
							<Label for="req-scoper">Who scopes it</Label>
							<select id="req-scoper" bind:value={target} class={field}>
								<option value="">{s.status === 'new' ? 'Choose an agent…' : 'Nobody (take it back)'}</option>
								{#each suggestions as x (x.member.id)}<option value={x.member.id}>{x.member.name} · {x.why}</option>{/each}
								{#each agents.filter((a) => !suggestions.some((x) => x.member.id === a.id)) as a (a.id)}<option value={a.id}>{a.name}{a.agent?.status === 'paused' ? ' (paused)' : ''}</option>{/each}
							</select>
							{#if snapshot.members.find((m) => m.id === target)?.agent?.status === 'paused'}<p class="text-sm/6 text-amber-200" role="alert">{nameOf(snapshot, target)} is paused and won't pick this up. Choose someone else or leave it not routed.</p>{/if}
						{:else if step === 'merge'}
							<Label for="req-into">Merge into</Label>
							<select id="req-into" bind:value={into} class={field}><option value="">Choose a request…</option>{#each others as o (o.id)}<option value={o.id}>{requestRef(o)} · {o.title}</option>{/each}</select>
							<p class="text-xs/5 text-gray-400">Its words are added to that request as a note, and you stop seeing this one in the queue.</p>
						{:else if step === 'approve'}
							<Label for="req-project">Which project?</Label>
							<select id="req-project" bind:value={projectId} class={field}><option value="">Choose a project…</option>{#each projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}</select>
							{#if projects.length === 0}<p class="text-sm/6 text-amber-200" role="alert">There is no project to put the tasks in. Create a project first.</p>{/if}
							<p class="text-sm/6 text-gray-200">These become tasks in {projectId ? (projects.find((p) => p.id === projectId)?.name ?? 'that project') : 'that project'}. Edit or remove any of them first.</p>
							{#if dropped > 0}<p class="text-xs/5 text-gray-400" data-testid="drafts-dropped">{dropped} {dropped === 1 ? 'draft names' : 'drafts name'} a team or person that doesn't apply there, so {dropped === 1 ? 'it is' : 'they are'} created without it.</p>{/if}
							<ul class="space-y-2" aria-label="Tasks to create">
								{#each drafts as d, i (i)}
									<li class="flex items-center gap-x-2">
										<input type="checkbox" bind:checked={d.keep} aria-label="Create task {i + 1}" />
										<Input bind:value={d.title} disabled={!d.keep} aria-label="Task {i + 1} title" autocomplete="off" />
									</li>
								{/each}
							</ul>
						{:else}
							<Label for="req-note">{step === 'changes' ? 'What should change' : 'Why'}</Label>
							<Input id="req-note" bind:value={note} autocomplete="off" />
						{/if}
						<div class="flex gap-x-2">
							<Button size="sm" disabled={busy || !canSubmit} onclick={submitStep}>
								{step === 'route' ? (target ? 'Send' : 'Take back') : step === 'merge' ? 'Merge' : step === 'approve' ? `Create ${kept.length} ${kept.length === 1 ? 'task' : 'tasks'}` : step === 'changes' ? 'Send back' : 'Decline'}
							</Button>
							<Button size="sm" variant="outline" onclick={close}>Cancel</Button>
						</div>
					</section>
				{/if}

				{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			</div>
		{:else if error}
			<p class="px-4 text-sm/6 text-red-400" role="alert">{error}</p>
		{/if}
	</SheetContent>
</Sheet>
