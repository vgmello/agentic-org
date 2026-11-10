<script lang="ts">
	import { tick, untrack } from 'svelte';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import BringInReview from '$lib/components/BringInReview.svelte';
	import { importApi } from '$lib/import/api';
	import { addResults, bringIn, owesWork, type BringInResult } from '$lib/import/bring-in';
	import { applyHints, type HintNotes } from '$lib/import/hints';
	import { browserLedger } from '$lib/import/ledger';
	import { markRemoved, normalize } from '$lib/import/review';
	import { buildRows, counts, type Row } from '$lib/import/rows';
	import { suggestProject, suggestTeams, whatHappens, withDefaultManagers, type ProjectDraft, type TeamDraft } from '$lib/import/teams';
	import type { Defaults, Hints, HrAgent, Scan } from '$lib/import/types';
	import { goto } from '$app/navigation';
	import { toAppError } from '$lib/office/api';
	import type { OrgSession } from '$lib/org/session';
	import type { Member, OrgSnapshot } from '$lib/proto';

	/** "Bring in an existing team" (our-org-in-the-app §4): where the team is today, a review, then Bring in. */
	let {
		open = $bindable(false),
		session,
		snapshot,
		organizationId,
		organization,
		officeId,
		ondone
	}: {
		open: boolean;
		session: OrgSession;
		/** The organization as it is: agents already in are marked, and the owner's name is not an agent. */
		snapshot: OrgSnapshot;
		organizationId: string;
		organization: string;
		officeId: string;
		/** Said when the dialog closes: what the run did (none if it never ran) and the messaging names of the agents it covered. */
		ondone: (result: { done: BringInResult | undefined; handles: string[] }) => void;
	} = $props();

	type Step = 'sources' | 'review' | 'running' | 'done';
	let step = $state<Step>('sources');
	let defaults = $state<Defaults | undefined>();
	let hrDir = $state<string | undefined>();
	let spoolDir = $state<string | undefined>();
	let hintsPath = $state<string | undefined>();
	let scan = $state<Scan | undefined>();
	let scanning = $state(false);
	let scanFailure = $state<string | undefined>();
	let hints = $state<Hints | undefined>();
	// A hints file the owner tried that could not be read; the earlier good one (if any) stays in use.
	let hintsFailure = $state<{ file: string; reason: string } | undefined>();
	// Each scan or file read carries a number: a slower, older one that finishes late is ignored.
	let latest = 0;

	let rows = $state<Row[]>([]);
	let drafts = $state<TeamDraft[]>([]);
	let offboarded = $state<HrAgent[]>([]);
	let project = $state<ProjectDraft | undefined>();
	let projectOn = $state(true);
	let hintNotes = $state<HintNotes | undefined>();

	let progress = $state('');
	// What every try of this import did together (a Try again adds to it), and the last try's problem.
	let result = $state<BringInResult | undefined>();
	let crashed = $state<string | undefined>();
	let ran = false;
	let reported = false;
	let busy = $derived(step === 'running');

	// A step that replaces the control the keyboard was on must not drop focus behind the dialog: it goes to the new step.
	let body = $state<HTMLElement | null>(null);
	let shownStep: Step | undefined;
	$effect(() => {
		const now = step;
		untrack(() => {
			if (shownStep && shownStep !== now) void tick().then(() => body?.focus());
			shownStep = now;
		});
	});

	$effect(() => {
		// Only opening starts over: a reload behind the dialog must not throw away the owner's edits.
		if (open) untrack(() => void start());
	});

	async function start() {
		latest += 1;
		step = 'sources';
		scan = undefined;
		scanFailure = undefined;
		scanning = false;
		hints = undefined;
		hintsPath = undefined;
		hintsFailure = undefined;
		result = undefined;
		crashed = undefined;
		ran = false;
		reported = false;
		try {
			const mine = latest;
			const found = await importApi.defaults();
			if (mine !== latest) return;
			defaults = found;
			hrDir = defaults.hr;
			spoolDir = defaults.messaging;
			if (hrDir || spoolDir) await rescan();
		} catch (e) {
			scanFailure = toAppError(e).message;
		}
	}

	async function rescan() {
		const mine = ++latest;
		scan = undefined; // a failed read must not leave the last folder's answer to be reviewed
		scanning = true;
		scanFailure = undefined;
		try {
			const read = await importApi.scan(hrDir, spoolDir);
			if (mine === latest) scan = read;
		} catch (e) {
			if (mine === latest) scanFailure = toAppError(e).message;
		} finally {
			if (mine === latest) scanning = false;
		}
	}

	async function choose(which: 'hr' | 'spool') {
		try {
			const picked = await importApi.pickFolder(which === 'hr' ? 'Choose the HR workspace' : 'Choose the messaging folder');
			if (!picked) return;
			if (which === 'hr') hrDir = picked;
			else spoolDir = picked;
			await rescan();
		} catch (e) {
			scanFailure = toAppError(e).message;
		}
	}

	async function chooseHints() {
		try {
			const picked = await importApi.pickFile('Choose the team hints file (teams.toml)');
			if (!picked) return;
			const mine = ++latest;
			const read = await importApi.readHints(picked).catch((e): Hints => ({ path: picked, error: toAppError(e).message, teams: [] }));
			if (mine !== latest) return;
			if (read.error) {
				hintsFailure = { file: fileName(picked) ?? picked, reason: read.error };
				return;
			}
			hintsFailure = undefined;
			hintsPath = picked;
			hints = read;
			if (step === 'review') applyLate(read);
		} catch (e) {
			hintsFailure = { file: 'the team hints file', reason: toAppError(e).message };
		}
	}

	/** Hints chosen after the review is open: put on the rows as they are now, so what the owner chose stays. */
	function applyLate(read: Hints) {
		const fitted = applyHints(rows, read, drafts);
		drafts = fitted.drafts;
		hintNotes = fitted.notes;
		const fixed = normalize(fitted.rows, drafts);
		drafts = fixed.drafts;
		rows = withDefaultManagers(fixed.rows, drafts);
		project = suggestProject(rows, drafts) ?? project;
	}

	const fileName = (path: string | undefined) => path?.split('/').filter(Boolean).at(-1);
	const ownerName = $derived(snapshot.members.find((m) => m.owner)?.name);
	const hrAgents = $derived(scan?.hr && !scan.hr.error ? scan.hr.agents : []);
	// The owner's own inbox is not a name to bring in.
	const names = $derived((scan?.messaging && !scan.messaging.error ? scan.messaging.names : []).filter((n) => ![defaults?.user, ownerName].some((o) => o && o.toLowerCase() === n.toLowerCase())));
	const notInHr = $derived(names.filter((n) => !hrAgents.some((a) => a.id.toLowerCase() === n.toLowerCase())).length);
	const projects = $derived(new Set(hrAgents.map((a) => a.project).filter(Boolean)));
	const found = $derived(hrAgents.length > 0 || names.length > 0);

	function build() {
		if (!scan) return;
		const exclude = [defaults?.user, ownerName].filter((n): n is string => !!n);
		const review = buildRows(scan, { exclude, members: snapshot.members as Member[] });
		const base = suggestTeams(review.rows);
		const fitted = hints ? applyHints(review.rows, hints, base) : { rows: review.rows, drafts: base, notes: undefined };
		drafts = fitted.drafts;
		// Who stays out (skipped, removed since an earlier import) is neither a manager nor a lead.
		const settled = normalize(markRemoved(fitted.rows, browserLedger(organizationId).load(), new Set(snapshot.members.map((m) => m.id))), drafts);
		drafts = settled.drafts;
		rows = withDefaultManagers(settled.rows, drafts);
		hintNotes = fitted.notes;
		offboarded = review.offboarded;
		project = suggestProject(rows, drafts);
		projectOn = !!project;
		step = 'review';
	}

	const c = $derived(counts(rows));
	const happens = $derived(whatHappens(rows, drafts, projectOn ? project : undefined, organization));
	// What an earlier try of this import left owing (it was closed after a failure): bringing in again finishes it.
	const owed = $derived(step === 'review' && owesWork(browserLedger(organizationId).load(), rows, drafts, snapshot));
	const bringLabel = $derived(c.ready === 0 && owed ? 'Finish bringing in' : `Bring in ${c.ready} ${c.ready === 1 ? 'agent' : 'agents'}${c.needDetails > 0 ? ` · ${c.needDetails} need details` : ''}`);
	/** The owner wants an agent that an earlier import made and they removed: forget that it was ours, so it is made again. */
	function bringBack(row: Row) {
		const store = browserLedger(organizationId);
		const ledger = store.load();
		delete ledger.agents[row.messagingName.toLowerCase()];
		store.save(ledger);
		rows = rows.map((r) => (r.key === row.key ? { ...r, removedEarlier: false } : r));
	}
	const roleFiles = $derived(scan?.hr && !scan.hr.error ? scan.hr.roles : []);

	async function run() {
		if (busy || (c.ready === 0 && !owed)) return;
		step = 'running';
		crashed = undefined;
		ran = true;
		try {
			const again = await bringIn(session, {
				rows,
				drafts,
				project: projectOn ? project : undefined,
				organizationId,
				officeId,
				ledger: browserLedger(organizationId),
				onProgress: (text) => (progress = text)
			});
			result = addResults(result, again);
		} catch (e) {
			crashed = toAppError(e).message;
		}
		step = 'done';
	}

	const summary = $derived.by(() => {
		if (!result) return '';
		const plural = (n: number, one: string) => `${n} ${n === 1 ? one : `${one}s`}`;
		const text = `${[plural(result.agents, 'agent'), plural(result.teams, 'team'), plural(result.projects, 'project')].join(', ')}.`;
		return result.paused > 0 ? `${text} ${plural(result.paused, 'agent')} paused.` : text;
	});
	const nothingNew = $derived(!!result && result.agents + result.teams + result.projects === 0 && result.failures.length === 0 && !crashed);

	/** Tell the page what happened, once, however the dialog is closed (its buttons, Escape, the X or a click outside). */
	function report() {
		if (reported || !ran) return;
		reported = true;
		ondone({ done: result, handles: result?.made ?? [] });
	}

	function close() {
		open = false;
		report();
	}

	function onOpenChange(next: boolean) {
		if (next) return;
		if (busy) open = true;
		else report();
	}
</script>

<Dialog bind:open {onOpenChange}>
	<DialogContent class="max-h-[90vh] w-full overflow-y-auto sm:max-w-[min(96vw,88rem)]" data-testid="bring-in">
		<DialogHeader>
			<DialogTitle>Bring in an existing team</DialogTitle>
			<DialogDescription>
				{#if step === 'sources'}Where is your team today? Scanning only reads: it writes nothing.
				{:else if step === 'review'}Review {rows.length} {rows.length === 1 ? 'agent' : 'agents'}. Nothing is created until you press Bring in.
				{:else if step === 'running'}{progress || 'Starting…'}
				{:else if crashed}Stopped.{:else if result && result.failures.length > 0}Not everything went through.{:else}Done.{/if}
			</DialogDescription>
		</DialogHeader>

		<div bind:this={body} tabindex="-1" class="outline-none">
		{#if step === 'sources'}
			<div class="space-y-3" data-testid="bring-in-sources">
				{#each [{ id: 'hr', title: 'HR workspace', path: hrDir, scanned: scan?.hr }, { id: 'spool', title: 'Messaging', path: spoolDir, scanned: scan?.messaging }] as source (source.id)}
					<div class="flex flex-wrap items-start gap-x-3 gap-y-1 rounded-lg bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10">
						<div class="min-w-0 flex-1">
							<p class="font-medium text-white">{source.title}</p>
							<p class="font-mono text-xs/5 text-gray-400 [overflow-wrap:anywhere]">{source.path ?? 'Not found in the usual places'}</p>
							{#if source.scanned?.error}
								<p class="mt-1 flex items-start gap-x-1 text-sm/6 text-red-300" role="alert"><CircleAlert class="mt-1 size-4 shrink-0" aria-hidden="true" /> {source.scanned.error}</p>
							{:else if source.id === 'hr' && scan?.hr}
								<p class="text-sm/6 text-gray-300">Found {hrAgents.length} {hrAgents.length === 1 ? 'agent' : 'agents'} in {projects.size} {projects.size === 1 ? 'project' : 'projects'}{projects.size === 1 ? ` (${[...projects][0]})` : ''}</p>
								{#if scan.hr.skipped.length > 0}<p class="text-xs/5 text-amber-200">{scan.hr.skipped.length + (scan.hr.skipped_more ?? 0)} records couldn't be read.</p>{/if}
							{:else if source.id === 'spool' && scan?.messaging}
								<p class="text-sm/6 text-gray-300">Found {names.length} {names.length === 1 ? 'name' : 'names'}{scan.hr && !scan.hr.error ? `, ${notInHr} not in the HR workspace` : ''}</p>
							{/if}
						</div>
						<Button type="button" variant="outline" size="sm" disabled={scanning} aria-label="Change the {source.title} folder" onclick={() => choose(source.id as 'hr' | 'spool')}>Change…</Button>
					</div>
				{/each}
				<div class="flex flex-wrap items-start gap-x-3 gap-y-1 rounded-lg bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10" data-testid="hints-source">
					<div class="min-w-0 flex-1">
						<p class="font-medium text-white">Team hints <span class="font-normal text-gray-400">(optional)</span></p>
						{#if hintsPath}
							<p class="font-mono text-xs/5 text-gray-400 [overflow-wrap:anywhere]">{hintsPath}</p>
							{#if hints}<p class="text-sm/6 text-gray-300">{hints.teams.length} {hints.teams.length === 1 ? 'team' : 'teams'} in {fileName(hintsPath)}</p>{/if}
						{:else}
							<p class="text-sm/6 text-gray-400">A teams.toml that says who is in which team.</p>
						{/if}
						{#if hintsFailure}<p class="mt-1 text-sm/6 text-amber-200" role="status">Couldn't read {hintsFailure.file}: {hintsFailure.reason} The import goes on without it.</p>{/if}
					</div>
					<Button type="button" variant="outline" size="sm" disabled={scanning} aria-label={hintsPath ? 'Change the team hints file' : 'Choose a team hints file'} onclick={chooseHints}>{hintsPath ? 'Change…' : 'Choose…'}</Button>
				</div>
				{#if scanning}<p class="flex items-center gap-x-2 text-sm/6 text-gray-300" role="status"><LoaderCircle class="size-4 animate-spin" aria-hidden="true" /> Reading…</p>{/if}
				{#if scanFailure}<p class="text-sm/6 text-red-300" role="alert">{scanFailure}</p>{/if}
				{#if scan && !scanning && !found && !scan.hr?.error && !scan.messaging?.error}
					<p class="text-sm/6 text-gray-300">No HR workspace or messaging found here. Choose a folder, or start empty.</p>
				{/if}
				{#if defaults && !hrDir && !spoolDir}
					<p class="text-sm/6 text-gray-300">No HR workspace or messaging found here. Choose a folder, or start empty.</p>
				{/if}
			</div>
		{:else if step === 'review' && scan}
			<BringInReview
				bind:rows
				bind:drafts
				bind:project
				bind:projectOn
				{scan}
				{offboarded}
				roles={roleFiles}
				{organization}
				hintsFile={hints ? fileName(hintsPath) : undefined}
				hintsNotes={hints ? hintNotes : undefined}
				{hintsFailure}
				alreadyThere={drafts.filter((d) => snapshot.teams.some((t) => t.name.trim().toLowerCase() === d.name.trim().toLowerCase())).map((d) => d.name)}
				{busy}
				members={snapshot.members}
				onbringback={bringBack}
				onchoosehints={chooseHints}
			/>
			<p class="text-sm/6 text-gray-300" data-testid="what-happens">{happens}</p>
		{:else if step === 'running'}
			<p class="flex items-center gap-x-2 text-sm/6 text-white" role="status" data-testid="bring-in-progress"><LoaderCircle class="size-4 animate-spin" aria-hidden="true" /> {progress}</p>
		{:else}
			<div class="space-y-3" data-testid="bring-in-done" role="status">
				{#if crashed}
					<p class="text-sm/6 text-red-300" role="alert">Bring in stopped: {crashed}</p>
				{:else if nothingNew}
					<p class="text-sm/6 text-white">Nothing new: everything you chose is already in {organization}.</p>
				{:else}
					<p class="text-base/7 font-semibold text-white">{summary}</p>
				{/if}
				{#if result && result.failures.length > 0}
					<div class="rounded-lg bg-red-500/10 p-3 text-sm/6 text-red-200 outline-1 -outline-offset-1 outline-red-400/30">
						<p class="font-medium">Not everything went through. Agents already added stay; Try again continues where it stopped.</p>
						<ul class="mt-1 list-disc pl-5">{#each result.failures as f, i (i)}<li class="[overflow-wrap:anywhere]">{f.what}: {f.error?.message}</li>{/each}</ul>
					</div>
				{/if}
			</div>
		{/if}
		</div>

		<DialogFooter class="sticky -bottom-6 -mx-6 -mb-6 border-t border-white/10 bg-popover px-6 py-4">
			{#if step === 'sources'}
				<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
				<Button type="button" disabled={!scan || scanning || !found} onclick={build}>Scan</Button>
			{:else if step === 'review'}
				<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
				<Button type="button" disabled={c.ready === 0 && !owed} onclick={run}>{bringLabel}</Button>
			{:else if step === 'done'}
				{#if (result && result.failures.length > 0) || crashed}<Button type="button" variant="outline" onclick={run}>Try again</Button>{/if}
				{#if result && result.failures.length === 0 && !nothingNew}<Button type="button" variant="outline" onclick={() => (close(), void goto('/activity'))}>View Activity</Button>{/if}
				<Button type="button" onclick={close}>{result && result.failures.length === 0 && !nothingNew ? 'View org chart' : 'Close'}</Button>
			{/if}
		</DialogFooter>
	</DialogContent>
</Dialog>
