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
	import { atName, atSpan, handleFor, KIND_WORD, requestRef, suggestScopers, withHandle, withoutHandle } from '$lib/work/requests-view';
	import type { Member } from '$lib/proto';

	/**
	 * New request (RQS-2 to RQS-5): the words are enough. The kind, project and who scopes it are suggested and can be
	 * changed or left; without the copilot the suggestion follows simple rules and says so.
	 */
	let {
		open = $bindable(false),
		snapshot,
		defaultProjectId,
		find,
		send,
		message,
		ondone,
		onadded,
		onmessaged
	}: {
		open: boolean;
		snapshot: OrgSnapshot;
		defaultProjectId?: string;
		/** The open requests that read like these words (`request.similar`): offered while typing (RQS-13). Without it nothing is offered. */
		find?: (text: string) => Promise<WorkItemSummary[]>;
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
	/** What the words' @handle says right now: a pick only stands while it says the same as when the pick was made. */
	const atKey = $derived(at ? (at.kind === 'exact' ? `exact:${at.member.id}` : `${at.kind}:${at.token}`) : '');
	const pickStands = $derived((pickedAt ?? '') === atKey && (picked === 'other' || (!!atMember && picked === atMember.id) || suggestions.some((s) => s.member.id === picked)));
	// While a handle is unresolved no one is chosen (nothing is drawn as checked that Send would not use), until a pick settles it.
	const unresolved = $derived(at?.kind === 'unknown' || at?.kind === 'partial');
	const choice = $derived(unresolved && !(picked !== undefined && pickStands) ? 'none' : ((pickStands ? picked : undefined) ?? atMember?.id ?? suggestions[0]?.member.id ?? 'none'));
	/** Added to a request the words become a note, which holds fewer characters than a request does. */
	const tooLong = $derived([...text].length > WORK_LIMITS.text);
	// After a pause in typing the office is asked which open requests read like the words (one call); up to 2 are offered.
	let similar = $state<WorkItemSummary[]>([]);
	$effect(() => {
		const lookup = open ? find : undefined;
		const needle = text.trim();
		// What was found stays on screen while the words change (the controls below do not jump), until the answer for the new words arrives or the words are too short.
		if (!lookup || [...needle].length < 8) {
			similar = [];
			return;
		}
		let live = true;
		const timer = setTimeout(() => {
			lookup([...needle].slice(0, 200).join('')).then(
				(found) => {
					if (live) similar = found.slice(0, 2);
				},
				() => {
					if (live) similar = [];
				}
			);
		}, 350);
		return () => {
			live = false;
			clearTimeout(timer);
		};
	});
	const scoperId = $derived(choice === 'none' ? undefined : choice === 'other' ? other || undefined : choice);
	const suggestion = $derived(suggestions.find((s) => s.member.id === scoperId));
	/** Whether the route is still the one the rule suggested: nothing picked, no @name. */
	const byRule = $derived(!!suggestion && !atMember && (picked === undefined || !pickStands));
	const warning = $derived.by(() => {
		const m = snapshot.members.find((x) => x.id === scoperId);
		return m?.kind === 'agent' && m.agent?.status === 'paused' ? `${m.name} is paused and won't pick this up. Choose someone else or Just save.` : undefined;
	});
	/** The words as they are saved: without the @handle when that is what routes it (decision 5). */
	const words = $derived(atMember && scoperId === atMember.id ? withoutHandle(text) : text);
	/** A handle at the start that no agent has: "No one called @x." Send is off until it is fixed or removed (RQS-4). */
	const unknownHandle = $derived(at?.kind === 'unknown' ? at.token : undefined);
	/** A handle that fits several agents, or is still half typed (`@sa`, `@juno`): not a route until one is picked. */
	const ambiguous = $derived(at?.kind === 'partial' ? at : undefined);
	const textError = $derived(touched && words.trim() === '' ? (atMember && text.trim() !== '' ? `Write what you need after @${at?.kind === 'exact' ? at.token : ''}.` : 'Write what you need.') : undefined);
	// Picking someone in the list instead settles it: the handle is then just words.
	const cannotSend = $derived((unknownHandle !== undefined || ambiguous !== undefined) && !(picked !== undefined && pickStands && scoperId !== undefined));
	const sendLabel = $derived(cannotSend ? 'Send' : confirmed && warning ? 'Send anyway' : scoperId ? `Send to ${nameOf(snapshot, scoperId)}` : 'Save');

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

	// The agents a half-typed @handle could be: ↑↓ move, Enter or Tab pick, Esc closes the list (and not the panel).
	let activeAt = $state(0);
	let atClosed = $state(false);
	// Where the caret is: the list belongs to the handle at the start and only shows while the caret is in or at the end of it.
	let caret = $state(0);
	const atToken = $derived(at?.kind === 'partial' ? at.token : undefined);
	$effect(() => {
		void atToken;
		activeAt = 0;
		atClosed = false;
	});
	const inHandle = $derived(caret <= (atSpan(text) ?? -1));
	const completions = $derived(at?.kind === 'partial' && !atClosed && inHandle ? at.matches : []);
	const trackCaret = (e: Event) => (caret = (e.currentTarget as HTMLTextAreaElement).selectionStart ?? 0);
	function onKey(e: KeyboardEvent) {
		// A key that is part of composing text (an input method) is not a command.
		if (e.isComposing || e.keyCode === 229) return;
		if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
			e.preventDefault();
			void submit(true);
			return;
		}
		if (completions.length === 0 || e.shiftKey || e.altKey || e.metaKey || e.ctrlKey) return;
		if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
			e.preventDefault();
			activeAt = (activeAt + (e.key === 'ArrowDown' ? 1 : completions.length - 1)) % completions.length;
		} else if (e.key === 'Enter' || e.key === 'Tab') {
			e.preventDefault();
			complete(completions[Math.min(activeAt, completions.length - 1)]);
		} else if (e.key === 'Escape') {
			e.preventDefault();
			e.stopPropagation();
			atClosed = true;
		}
	}

	function complete(m: Member) {
		text = withHandle(text, m);
		picked = undefined;
		pickedAt = undefined;
		// The caret stays at the end of the handle, where the owner was, not at the end of the words.
		const end = atSpan(text) ?? text.length;
		queueMicrotask(() => {
			field?.focus();
			field?.setSelectionRange(end, end);
			caret = end;
		});
	}

	/** The words become a note on a request that was already there. */
	async function addTo(request: WorkItemSummary) {
		if (busy) return;
		busy = true;
		error = undefined;
		try {
			await send({ method: 'work.comment', item_id: request.id, comment_id: newId(), body: at?.kind === 'exact' ? withoutHandle(text) : text });
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
		if (busy || !message || !scoperId || words.trim() === '' || cannotSend) return;
		busy = true;
		error = undefined;
		try {
			await message(scoperId, words);
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
		if (busy || words.trim() === '' || (route && cannotSend)) return;
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
				text: route || !atMember ? words : text,
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
	<DialogContent class="max-h-[calc(100dvh-2rem)] w-[calc(100vw-2rem)] max-w-none grid-rows-[auto_minmax(0,1fr)] overflow-hidden sm:max-w-xl">
		<DialogHeader>
			<DialogTitle text={'New request'} />
			<DialogDescription>A request isn't a task yet. Whoever scopes it turns it into a feature you can approve.</DialogDescription>
		</DialogHeader>
		<form class="flex min-h-0 flex-col gap-4" onsubmit={(e) => (e.preventDefault(), void submit(true))} novalidate>
			<!-- The body scrolls when the window or the text is too small for it; the buttons below it stay in view. -->
			<div class="min-h-0 flex-1 space-y-4 overflow-y-auto px-0.5 py-0.5">
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
					onkeydown={onKey}
					onkeyup={trackCaret}
					onclick={trackCaret}
					oninput={trackCaret}
					onselect={trackCaret}
					role="combobox"
					aria-autocomplete="list"
					aria-describedby={cannotSend ? 'handle-problem' : undefined}
					aria-expanded={completions.length > 0}
					aria-haspopup="listbox"
					aria-controls={completions.length > 0 ? 'at-list' : undefined}
					aria-activedescendant={completions.length > 0 ? `at-opt-${Math.min(activeAt, completions.length - 1)}` : undefined}
					class="field-sizing-content max-h-72 min-h-28 w-full rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400"
				></textarea>
				{#if textError}<p class="text-sm/6 text-red-400" role="alert">{textError}</p>{/if}
				{#if cannotSend}<p id="handle-problem" class="text-sm/6 text-amber-200 [overflow-wrap:anywhere]" aria-live="polite" data-testid="no-such-handle">{unknownHandle !== undefined ? `No one called @${unknownHandle}. Pick someone from the list or remove it.` : `@${ambiguous?.token ?? ''} could be ${ambiguous?.matches.map((m) => m.name).join(', ') ?? 'several agents'}. Pick one from the list or remove it.`}</p>{/if}
				{#if completions.length > 0}
					<ul id="at-list" role="listbox" aria-label="Agents you could mean" class="flex flex-wrap gap-2" data-testid="at-matches">
						{#each completions as m, i (m.id)}
							<!-- svelte-ignore a11y_click_events_have_key_events -->
							<li id="at-opt-{i}" role="option" aria-selected={i === Math.min(activeAt, completions.length - 1)} class="cursor-pointer rounded-md px-2 py-1 text-sm/6 text-gray-100 hover:bg-white/10 {i === Math.min(activeAt, completions.length - 1) ? 'bg-white/15 outline-2 outline-indigo-400' : 'bg-white/5'}" onmousedown={(e) => e.preventDefault()} onclick={() => complete(m)}>{m.name} <span class="text-gray-300">@{handleFor(m)}{m.agent?.title ? ` · ${m.agent.title}` : ''}{m.agent?.status === 'paused' ? ' · paused' : ''}</span></li>
						{/each}
					</ul>
					<p class="text-xs/5 text-gray-400">↑↓ to choose, Enter or Tab to pick, Esc to close the list.</p>
				{/if}
				<div aria-live="polite">
				{#if similar.length > 0}
					<ul class="space-y-1 rounded-md bg-white/5 px-3 py-2 text-sm/6" aria-label="Similar requests" data-testid="similar">
						{#each similar as r (r.id)}
							<li class="flex flex-wrap items-center gap-x-2 text-gray-200"><span>Looks like {requestRef(r)}: <span class="[overflow-wrap:anywhere]">{r.title}</span></span><Button type="button" size="sm" variant="outline" disabled={busy || tooLong} aria-label="Add to {requestRef(r)}" title={tooLong ? `A note can be ${WORK_LIMITS.text.toLocaleString('en-US')} characters at most.` : undefined} onclick={() => void addTo(r)}>Add to {requestRef(r)}</Button></li>
						{/each}
					</ul>
				{/if}
				</div>
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
						<input type="radio" name="scoper" value={atMember.id} checked={choice === atMember.id} onclick={() => ((picked = atMember.id), (pickedAt = atKey), (confirmed = false))} class="mt-1.5" />
						<span><span class="font-semibold text-white">{atMember.name}</span>, you wrote @{at?.kind === 'exact' ? at.token : ''}</span>
					</label>
				{/if}
				{#each suggestions as s (s.member.id)}
					<label class="flex items-start gap-2 text-sm/6 text-gray-200">
						<input type="radio" name="scoper" value={s.member.id} checked={choice === s.member.id} onclick={() => ((picked = s.member.id), (pickedAt = atKey), (confirmed = false))} class="mt-1.5" />
						<span><span class="font-semibold text-white">{s.member.name}</span>, {s.why}</span>
					</label>
				{/each}
				<label class="flex items-start gap-2 text-sm/6 text-gray-200">
					<input type="radio" name="scoper" value="other" checked={choice === 'other'} onclick={() => ((picked = 'other'), (pickedAt = atKey), (confirmed = false))} class="mt-1.5" />
					<span class="flex flex-wrap items-center gap-2">Someone else…
						<select aria-label="Someone else" bind:value={other} onchange={() => ((picked = 'other'), (pickedAt = atKey), (confirmed = false))} class="rounded-md bg-gray-800 px-2 py-1 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
							<option value="">Choose…</option>
							{#each agents as a (a.id)}<option value={a.id}>{a.name}{a.agent?.status === 'paused' ? ' (paused)' : ''}</option>{/each}
						</select>
					</span>
				</label>
			</fieldset>

			{#if message && !scoperId}<p id="message-hint" class="text-xs/5 text-gray-400" data-testid="message-hint">Choose who to message.</p>{/if}
			{#if warning}<p class="rounded-md bg-amber-500/10 px-3 py-2 text-sm/6 text-amber-100" role="alert" data-testid="route-warning">{warning}</p>{/if}
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			</div>
			<DialogFooter class="shrink-0">
				{#if asking}
					<p class="mr-auto self-center text-sm/6 text-gray-200" role="alert" data-testid="keep-draft">Keep as a draft?</p>
					<Button type="button" variant="outline" onclick={back}>Back</Button>
					<Button type="button" variant="outline" onclick={() => ((asking = false), (text = ''), (open = false))}>Discard</Button>
					<Button type="button" disabled={busy} onclick={() => void submit(false)}>Keep</Button>
				{:else}
					<Button type="button" variant="outline" disabled={busy} onclick={() => wantClose() && (open = false)}>Cancel</Button>
					{#if message}<Button type="button" variant="outline" disabled={busy || !scoperId || cannotSend} aria-describedby={scoperId ? undefined : 'message-hint'} onclick={() => void sendAsMessage()}>Message instead</Button>{/if}
					<Button type="button" variant="outline" disabled={busy} onclick={() => void submit(false)}>Just save</Button>
					<Button type="submit" disabled={busy || cannotSend} aria-describedby={cannotSend ? 'handle-problem' : undefined}>{sendLabel}</Button>
				{/if}
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
