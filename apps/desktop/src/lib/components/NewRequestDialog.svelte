<script lang="ts">
	import { untrack } from 'svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { newId } from '$lib/org/commands';
	import { isFormer } from '$lib/org/model';
	import type { OrgSnapshot, RequestKind, WorkAccepted, WorkItemSummary } from '$lib/proto';
	import { REQUEST_LIMITS, WORK_LIMITS } from '$lib/proto';
	import type { WorkDraft } from '$lib/work';
	import { nameOf } from '$lib/work/people';
	import { atName, handleFor, KIND_WORD, requestRef, similarRequests, suggestScopers, withHandle } from '$lib/work/requests-view';
	import type { Member } from '$lib/proto';

	/**
	 * New request (RQS-2 to RQS-5): the words are enough. The kind, project and who scopes it are suggested and can be
	 * changed or left; without the copilot the suggestion follows simple rules and says so.
	 */
	let {
		open = $bindable(false),
		snapshot,
		defaultProjectId,
		items = [],
		send,
		message,
		ondone,
		onadded,
		onmessaged
	}: {
		open: boolean;
		snapshot: OrgSnapshot;
		defaultProjectId?: string;
		/** The requests there are: the open ones that read alike are offered while typing (RQS-13). */
		items?: WorkItemSummary[];
		send: (draft: WorkDraft) => Promise<WorkAccepted>;
		/** Send the words to a member as an ordinary message instead (RQS-4). Without it there is no such button. */
		message?: (memberId: string, text: string) => Promise<void>;
		/** Saved: the request as the office numbered it, and who it was sent to, if anyone. */
		ondone: (created: { ref: string; sentTo: string | undefined }) => void;
		/** The words were added to a request that was already there. */
		onadded?: (ref: string) => void;
		/** The words went as a message. */
		onmessaged?: (to: string) => void;
	} = $props();

	type Kind = Exclude<RequestKind, 'unknown'>;
	const KINDS = Object.keys(KIND_WORD) as Kind[];

	let text = $state('');
	let kind = $state<Kind>('feature');
	let projectId = $state('');
	// What the owner chose for who scopes it: a suggestion's member id, 'other' (with `other`), or undefined: the first suggestion. (Just save is the way to queue it unrouted.)
	let picked = $state<string | undefined>();
	// Which @name was in the words when the pick was made: a pick made before the owner typed another @name does not stand.
	let pickedAt = $state<string | undefined>();
	let other = $state('');
	let touched = $state(false);
	let error = $state<string | undefined>();
	let busy = $state(false);
	let confirmed = $state(false);
	// Closing with words typed asks whether to keep them (Esc, the close button and Cancel all end here).
	let asking = $state(false);
	let field = $state<HTMLTextAreaElement | undefined>();

	$effect(() => {
		if (open) {
			untrack(() => {
				text = '';
				kind = 'feature';
				projectId = defaultProjectId ?? '';
				picked = undefined;
				pickedAt = undefined;
				other = '';
				touched = false;
				error = undefined;
				busy = false;
				confirmed = false;
				asking = false;
			});
			// The words come first: the field has the focus as soon as the panel opens (RQS-1).
			queueMicrotask(() => field?.focus());
		}
	});

	const projects = $derived((snapshot.projects ?? []).filter((p) => !p.archived));
	const suggestions = $derived(suggestScopers(snapshot, kind, projectId || undefined));
	const agents = $derived(snapshot.members.filter((m) => m.kind === 'agent' && !isFormer(m)));
	// A pick that the suggestions no longer hold (the kind or project changed) is not a pick any more.
	// Words that start with @handle send it to that agent without a suggestion (RQS-4), unless the owner then picks someone.
	const at = $derived(atName(text, snapshot));
	const atMember = $derived(at?.kind === 'exact' ? at.member : undefined);
	const pickStands = $derived(pickedAt === atMember?.id && (picked === 'other' || (!!atMember && picked === atMember.id) || suggestions.some((s) => s.member.id === picked)));
	const choice = $derived((pickStands ? picked : undefined) ?? atMember?.id ?? suggestions[0]?.member.id ?? 'none');
	/** Added to a request the words become a note, which holds fewer characters than a request does. */
	const tooLong = $derived([...text].length > WORK_LIMITS.text);
	const similar = $derived(text.trim().length >= 12 ? similarRequests(text, items) : []);
	const scoperId = $derived(choice === 'none' ? undefined : choice === 'other' ? other || undefined : choice);
	const suggestion = $derived(suggestions.find((s) => s.member.id === scoperId));
	/** Whether the route is still the one the rule suggested: nothing picked, no @name. */
	const byRule = $derived(!!suggestion && !atMember && (picked === undefined || !pickStands));
	const warning = $derived.by(() => {
		const m = snapshot.members.find((x) => x.id === scoperId);
		return m?.kind === 'agent' && m.agent?.status === 'paused' ? `${m.name} is paused and won't pick this up. Choose someone else or Just save.` : undefined;
	});
	const textError = $derived(touched && text.trim() === '' ? 'Write what you need.' : undefined);
	const sendLabel = $derived(confirmed && warning ? 'Send anyway' : scoperId ? `Send to ${nameOf(snapshot, scoperId)}` : 'Save');

	/** Whether closing now should ask first: words typed, nothing in flight. */
	function wantClose(): boolean {
		if (busy) return false;
		if (text.trim() === '') return true;
		asking = true;
		return false;
	}
	/** The dialog asks to open or close (Esc, the X, a click outside): closing with words typed asks first. */
	function setOpen(next: boolean) {
		if (next) open = true;
		else if (asking) back();
		else if (wantClose()) open = false;
	}
	const back = () => ((asking = false), queueMicrotask(() => field?.focus()));

	function complete(m: Member) {
		text = withHandle(text, m);
		picked = undefined;
		pickedAt = undefined;
		queueMicrotask(() => field?.focus());
	}

	/** The words become a note on a request that was already there. */
	async function addTo(request: WorkItemSummary) {
		if (busy) return;
		busy = true;
		error = undefined;
		try {
			await send({ method: 'work.comment', item_id: request.id, comment_id: newId(), body: text });
			text = '';
			open = false;
			onadded?.(requestRef(request));
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}

	/** The words go to the member as an ordinary message, not as a request (RQS-4). */
	async function sendAsMessage() {
		touched = true;
		if (busy || !message || !scoperId || text.trim() === '') return;
		busy = true;
		error = undefined;
		try {
			await message(scoperId, text);
			const to = nameOf(snapshot, scoperId);
			text = '';
			open = false;
			onmessaged?.(to);
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}

	async function submit(route: boolean) {
		touched = true;
		if (busy || text.trim() === '') return;
		if (route && warning && !confirmed) {
			confirmed = true;
			return;
		}
		busy = true;
		error = undefined;
		const to = route ? scoperId : undefined;
		try {
			asking = false;
			const accepted = await send({
				method: 'request.create',
				item_id: newId(),
				text,
				request_kind: kind,
				project_id: projectId || undefined,
				scoper_id: to,
				route_reason: to && byRule ? suggestion?.why : undefined,
				// A route the owner did not touch is the rule's; one they picked is theirs, even among the suggestions.
				routed_by: to ? (byRule ? 'rule' : 'person') : undefined
			});
			// Nothing left to lose: closing must not ask again.
			text = '';
			open = false;
			ondone({ ref: requestRef(accepted.items[0]), sentTo: to ? nameOf(snapshot, to) : undefined });
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open={() => open, setOpen}>
	<DialogContent class="sm:max-w-xl">
		<DialogHeader>
			<DialogTitle>New request</DialogTitle>
			<DialogDescription>A request isn't a task yet. Whoever scopes it turns it into a feature you can approve.</DialogDescription>
		</DialogHeader>
		<form class="space-y-4" onsubmit={(e) => (e.preventDefault(), void submit(true))} novalidate>
			<div class="space-y-2">
				<Label for="new-request-text">What do you need?</Label>
				<textarea
					id="new-request-text"
					bind:this={field}
					bind:value={text}
					rows="5"
					maxlength={REQUEST_LIMITS.text * 2}
					disabled={busy}
					aria-invalid={textError ? true : undefined}
					onkeydown={(e) => (e.metaKey || e.ctrlKey) && e.key === 'Enter' && (e.preventDefault(), void submit(true))}
					class="field-sizing-content max-h-72 min-h-28 w-full rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400"
				></textarea>
				{#if textError}<p class="text-sm/6 text-red-400" role="alert">{textError}</p>{/if}
				{#if at?.kind === 'partial'}
					<ul class="flex flex-wrap gap-2" aria-label="Agents you could mean" data-testid="at-matches">
						{#each at.matches as m (m.id)}
							<li><button type="button" class="rounded-md bg-white/5 px-2 py-1 text-sm/6 text-gray-200 hover:bg-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => complete(m)}>{m.name} <span class="text-gray-400">@{handleFor(m)}</span></button></li>
						{/each}
					</ul>
				{/if}
				{#if similar.length > 0}
					<ul class="space-y-1 rounded-md bg-white/5 px-3 py-2 text-sm/6" aria-label="Similar requests" data-testid="similar">
						{#each similar as r (r.id)}
							<li class="flex flex-wrap items-center gap-x-2 text-gray-200"><span>Looks like {requestRef(r)}: <span class="[overflow-wrap:anywhere]">{r.title}</span></span><Button type="button" size="sm" variant="outline" disabled={busy || tooLong} aria-label="Add to {requestRef(r)}" title={tooLong ? `A note can be ${WORK_LIMITS.text.toLocaleString('en-US')} characters at most.` : undefined} onclick={() => void addTo(r)}>Add to {requestRef(r)}</Button></li>
						{/each}
					</ul>
				{/if}
			</div>

			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label for="new-request-kind">Kind</Label>
					<select id="new-request-kind" bind:value={kind} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
						{#each KINDS as k (k)}<option value={k}>{KIND_WORD[k]}</option>{/each}
					</select>
				</div>
				<div class="space-y-2">
					<Label for="new-request-project">Project <span class="font-normal text-gray-400">(optional)</span></Label>
					<select id="new-request-project" bind:value={projectId} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
						<option value="">No project yet</option>
						{#each projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
					</select>
				</div>
			</div>

			<fieldset class="space-y-2" disabled={busy}>
				<legend class="flex items-center gap-2 text-sm/6 font-medium text-white">Who scopes it {#if atMember && (picked === undefined || !pickStands)}<Badge variant="secondary" data-testid="by-at">You wrote @{at?.kind === 'exact' ? at.token : ''}</Badge>{:else if suggestions.length > 0 && (picked === undefined || !pickStands)}<Badge variant="secondary" data-testid="by-rule">Suggested by rule</Badge>{/if}</legend>
				{#if atMember && !suggestions.some((x) => x.member.id === atMember.id)}
					<label class="flex items-start gap-2 text-sm/6 text-gray-200">
						<input type="radio" name="scoper" value={atMember.id} checked={choice === atMember.id} onchange={() => ((picked = atMember.id), (pickedAt = atMember.id), (confirmed = false))} class="mt-1.5" />
						<span><span class="font-semibold text-white">{atMember.name}</span>, you wrote @{at?.kind === 'exact' ? at.token : ''}</span>
					</label>
				{/if}
				{#each suggestions as s (s.member.id)}
					<label class="flex items-start gap-2 text-sm/6 text-gray-200">
						<input type="radio" name="scoper" value={s.member.id} checked={choice === s.member.id} onchange={() => ((picked = s.member.id), (pickedAt = atMember?.id), (confirmed = false))} class="mt-1.5" />
						<span><span class="font-semibold text-white">{s.member.name}</span>, {s.why}</span>
					</label>
				{/each}
				<label class="flex items-start gap-2 text-sm/6 text-gray-200">
					<input type="radio" name="scoper" value="other" checked={choice === 'other'} onchange={() => ((picked = 'other'), (pickedAt = atMember?.id), (confirmed = false))} class="mt-1.5" />
					<span class="flex flex-wrap items-center gap-2">Someone else…
						<select aria-label="Someone else" bind:value={other} onchange={() => ((picked = 'other'), (pickedAt = atMember?.id), (confirmed = false))} class="rounded-md bg-gray-800 px-2 py-1 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
							<option value="">Choose…</option>
							{#each agents as a (a.id)}<option value={a.id}>{a.name}{a.agent?.status === 'paused' ? ' (paused)' : ''}</option>{/each}
						</select>
					</span>
				</label>
			</fieldset>

			{#if warning}<p class="rounded-md bg-amber-500/10 px-3 py-2 text-sm/6 text-amber-100" role="alert" data-testid="route-warning">{warning}</p>{/if}
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			<DialogFooter>
				{#if asking}
					<p class="mr-auto self-center text-sm/6 text-gray-200" role="alert" data-testid="keep-draft">Keep as a draft?</p>
					<Button type="button" variant="outline" onclick={back}>Back</Button>
					<Button type="button" variant="outline" onclick={() => ((asking = false), (text = ''), (open = false))}>Discard</Button>
					<Button type="button" disabled={busy} onclick={() => void submit(false)}>Keep</Button>
				{:else}
					<Button type="button" variant="outline" disabled={busy} onclick={() => wantClose() && (open = false)}>Cancel</Button>
					{#if message}<Button type="button" variant="outline" disabled={busy || !scoperId} title={scoperId ? undefined : 'Choose who to message first.'} onclick={() => void sendAsMessage()}>Message instead</Button>{/if}
					<Button type="button" variant="outline" disabled={busy} onclick={() => void submit(false)}>Just save</Button>
					<Button type="submit" disabled={busy}>{sendLabel}</Button>
				{/if}
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
