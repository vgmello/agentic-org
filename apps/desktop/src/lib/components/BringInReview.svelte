<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { runtimeLabel } from '$lib/org/agents';
	import type { AgentRuntime } from '$lib/proto';
	import type { HintNotes } from '$lib/import/hints';
	import { addTeam, filterRows, FILTERS, normalize, patchRow, removeTeam, rowNote, setParent, staying, type RowFilter } from '$lib/import/review';
	import { chooseRole, chooseTitle, OWNER, roleWords, type Row } from '$lib/import/rows';
	import { teamsToCreate, withDefaultManagers, type ProjectDraft, type TeamDraft } from '$lib/import/teams';
	import type { HrAgent, Scan } from '$lib/import/types';

	/** The review (our-org-in-the-app §4): one row per agent, the teams, and the project. Nothing is written until Bring in. */
	let {
		rows = $bindable(),
		drafts = $bindable(),
		project = $bindable(),
		projectOn = $bindable(),
		scan,
		offboarded,
		roles,
		organization,
		hintsFile,
		hintsNotes,
		hintsFailure,
		alreadyThere,
		busy,
		onchoosehints
	}: {
		rows: Row[];
		drafts: TeamDraft[];
		project: ProjectDraft | undefined;
		projectOn: boolean;
		scan: Scan;
		offboarded: HrAgent[];
		/** The role files' names, for the Role choices. */
		roles: string[];
		organization: string;
		/** The team hints file in use (its name) and what it could not place. */
		hintsFile: string | undefined;
		hintsNotes: HintNotes | undefined;
		/** A file the owner tried that could not be read (the earlier good one, if any, stays in use). */
		hintsFailure: { file: string; reason: string } | undefined;
		/** Names of planned teams that the organization already has (they are left as they are). */
		alreadyThere: string[];
		busy: boolean;
		onchoosehints: () => void;
	} = $props();

	let filter = $state<RowFilter>('all');
	let editingTeams = $state(false);
	let newTeamName = $state('');
	const shown = $derived(filterRows(rows, filter));
	const catalog = $derived(roles.map((slug) => roleWords(slug)!).filter(Boolean));
	const willCreate = $derived(teamsToCreate(rows, drafts));
	const teamName = (key: string) => drafts.find((d) => d.key === key)?.name ?? key;
	const teamOf = (row: Row) => (row.teamKey ? (drafts.find((d) => d.key === row.teamKey!.trim().toLowerCase())?.key ?? '') : '');
	const select = 'w-full min-w-0 rounded-md bg-gray-800 px-2 py-1.5 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400 disabled:opacity-50';
	const SOURCES = { both: 'HR + messaging', hr: 'HR only', messaging: 'Messaging only' } as const;
	const RUNTIMES = ['claude_code', 'codex', 'antigravity'] as const;
	const runtimeValue = (r: Row) => (typeof r.runtime === 'string' ? r.runtime : '');
	// Anyone staying in the import can lead (one still to be filled in will be by the time it runs).
	const leadChoices = (team: TeamDraft) => rows.filter((r) => staying(r) && r.teamKey?.trim().toLowerCase() === team.key);

	const setRole = (r: Row, role: string) => ((rows = patchRow(rows, r.key, (x) => (role ? chooseRole(x, role) : { ...x, role: undefined, roleSuggested: false }))), refresh());
	const setTitle = (r: Row, title: string) => ((rows = patchRow(rows, r.key, (x) => chooseTitle(x, title, roles))), refresh());
	const setRuntime = (r: Row, value: string) => ((rows = patchRow(rows, r.key, { runtime: (value || undefined) as AgentRuntime | undefined })), refresh());
	// The default manager follows the teams until the owner sets one by hand.
	function refresh() {
		const fixed = normalize(rows, drafts);
		drafts = fixed.drafts;
		rows = withDefaultManagers(fixed.rows, drafts);
	}
	const setTeam = (r: Row, key: string) => ((rows = patchRow(rows, r.key, { teamKey: key || undefined })), refresh());
	const setManager = (r: Row, key: string) => (rows = patchRow(rows, r.key, { managerKey: key || OWNER, managerSet: true }));
	const setChoice = (r: Row, choice: string) => ((rows = patchRow(rows, r.key, { choice: choice as Row['choice'] })), refresh());
	const setHandle = (r: Row, name: string) => (rows = patchRow(rows, r.key, { messagingName: name.trim() }));
	const rename = (d: TeamDraft, name: string) => (drafts = drafts.map((x) => (x.key === d.key ? { ...x, name: name.trim() || x.name } : x)));
	const setLead = (d: TeamDraft, lead: string) => ((drafts = drafts.map((x) => (x.key === d.key ? { ...x, leadKey: lead || undefined } : x))), refresh());
	function remove(d: TeamDraft) {
		const out = removeTeam(drafts, rows, d.key);
		drafts = out.drafts;
		rows = out.rows;
		refresh();
	}
	function add() {
		drafts = addTeam(drafts, newTeamName);
		newTeamName = '';
	}
	const managers = $derived(rows.filter(staying));
	const needsName = (r: Row) => !!rowNote(r, scan, organization) && rowNote(r, scan, organization)!.tone === 'bad';
</script>

<div class="space-y-5" data-testid="bring-in-review">
	<div class="flex flex-wrap items-center gap-3">
		<label class="flex items-center gap-x-2 text-sm/6 text-gray-300">
			Filter
			<select bind:value={filter} class="rounded-md bg-gray-800 px-2 py-1 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400" aria-label="Filter rows">
				{#each FILTERS as f (f.id)}<option value={f.id}>{f.label}</option>{/each}
			</select>
		</label>
	</div>

	<div class="overflow-x-auto rounded-lg outline-1 -outline-offset-1 outline-white/10">
		<table class="w-full min-w-[68rem] table-fixed text-left text-sm/6" aria-label="Agents to bring in">
			<thead class="bg-white/5 text-xs/5 text-gray-400">
				<tr>
					<th scope="col" class="w-[17%] px-3 py-2 font-medium">Agent</th>
					<th scope="col" class="w-[15%] px-3 py-2 font-medium">Role</th>
					<th scope="col" class="w-[19%] px-3 py-2 font-medium">Title</th>
					<th scope="col" class="w-[13%] px-3 py-2 font-medium">Runs on</th>
					<th scope="col" class="w-[13%] px-3 py-2 font-medium">Team</th>
					<th scope="col" class="w-[13%] px-3 py-2 font-medium">Reports to</th>
					<th scope="col" class="w-[10%] px-3 py-2 font-medium">Bring in</th>
				</tr>
			</thead>
			<tbody class="divide-y divide-white/5">
				{#each shown as row (row.key)}
					{@const note = rowNote(row, scan, organization)}
					{@const settled = !!row.existingMemberId}
					<tr data-testid="import-row" data-row={row.key} class={row.choice === 'skip' ? 'opacity-50' : ''}>
						<td class="px-3 py-2 align-top">
							<p class="font-medium text-white [overflow-wrap:anywhere]">{row.name}</p>
							{#if needsName(row)}
								<Input aria-label="Messaging name for {row.name}" value={row.messagingName} onchange={(e) => setHandle(row, e.currentTarget.value)} class="mt-1 font-mono" disabled={busy} />
							{:else}
								<p class="font-mono text-xs/5 text-gray-400 [overflow-wrap:anywhere]">{row.messagingName}</p>
							{/if}
							<p class="text-xs/5 text-gray-400">{SOURCES[row.source]}</p>
							{#if note}<p class="mt-1 max-w-64 text-xs/5 {note.tone === 'bad' ? 'text-red-300' : note.tone === 'warn' ? 'text-amber-200' : 'text-gray-300'}" data-testid="import-note">{note.text}</p>{/if}
						</td>
						<td class="px-3 py-2 align-top">
							<select aria-label="Role for {row.name}" class={select} value={row.role ?? ''} disabled={busy || settled} onchange={(e) => setRole(row, e.currentTarget.value)}>
								<option value="">Role?</option>
								{#each catalog as r (r)}<option value={r}>{r}</option>{/each}
								{#if row.role && !catalog.includes(row.role)}<option value={row.role}>{row.role}</option>{/if}
							</select>
							{#if row.roleSuggested}<p class="mt-1 text-xs/5 text-gray-400">Suggested</p>{/if}
						</td>
						<td class="px-3 py-2 align-top">
							<Input aria-label="Title for {row.name}" value={row.title ?? ''} onchange={(e) => setTitle(row, e.currentTarget.value)} disabled={busy || settled} placeholder="Title" />
						</td>
						<td class="px-3 py-2 align-top">
							<select aria-label="Runtime for {row.name}" class={select} value={runtimeValue(row)} disabled={busy || settled} onchange={(e) => setRuntime(row, e.currentTarget.value)}>
								<option value="">Runtime?</option>
								{#each RUNTIMES as r (r)}<option value={r}>{runtimeLabel(r)}</option>{/each}
							</select>
							{#if row.hr && row.runtime === 'claude_code'}<p class="mt-1 text-xs/5 text-gray-400">from HR</p>{/if}
						</td>
						<td class="px-3 py-2 align-top">
							<select aria-label="Team for {row.name}" class={select} value={teamOf(row)} disabled={busy || settled} onchange={(e) => setTeam(row, e.currentTarget.value)}>
								<option value="">Not in a team</option>
								{#each drafts as d (d.key)}<option value={d.key}>{d.name}</option>{/each}
							</select>
						</td>
						<td class="px-3 py-2 align-top">
							<select aria-label="Reports to for {row.name}" class={select} value={row.managerKey} disabled={busy || settled} onchange={(e) => setManager(row, e.currentTarget.value)}>
								<option value={OWNER}>The owner</option>
								{#each managers.filter((m) => m.key !== row.key) as m (m.key)}<option value={m.key}>{m.name}</option>{/each}
							</select>
						</td>
						<td class="px-3 py-2 align-top">
							<select aria-label="Bring in {row.name}" class={select} value={row.choice} disabled={busy || settled} onchange={(e) => setChoice(row, e.currentTarget.value)}>
								<option value="active">Active</option>
								<option value="paused">Paused</option>
								<option value="skip">Skip</option>
							</select>
						</td>
					</tr>
				{:else}
					<tr><td colspan="7" class="px-3 py-6 text-center text-gray-400">{rows.length === 0 ? 'No agents found in what you chose.' : 'No rows match this filter.'}</td></tr>
				{/each}
			</tbody>
		</table>
	</div>

	{#if offboarded.length > 0}
		<details class="rounded-lg bg-white/5 px-3 py-2 text-sm/6 outline-1 -outline-offset-1 outline-white/10">
			<summary class="cursor-pointer text-gray-300">Offboarded in HR ({offboarded.length})</summary>
			<ul class="mt-2 list-disc pl-5 text-gray-400">{#each offboarded as a (a.id)}<li class="[overflow-wrap:anywhere]">{a.name ?? a.id}: kept as a record, not brought in.</li>{/each}</ul>
		</details>
	{/if}

	<section class="space-y-2 rounded-lg bg-white/5 p-3 text-sm/6 outline-1 -outline-offset-1 outline-white/10" aria-label="Teams">
		<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
			<p class="text-white" data-testid="teams-line">
				Teams: {#if hintsFile}from {hintsFile} ({drafts.length}){:else}{willCreate.length === 0 ? 'none' : `create ${willCreate.map((t) => t.name).join(', ')}`}{/if}
			</p>
			<Button type="button" variant="outline" size="sm" disabled={busy} onclick={onchoosehints}>{hintsFile ? 'Change team hints…' : 'Team hints (optional)…'}</Button>
			<Button type="button" variant="outline" size="sm" disabled={busy} onclick={() => (editingTeams = !editingTeams)} aria-expanded={editingTeams}>{editingTeams ? 'Done' : 'Edit…'}</Button>
		</div>
		{#each alreadyThere as t (t)}<p class="text-gray-300" data-testid="team-already">{t} is already in {organization}: left as it is.</p>{/each}
		{#if hintsFailure}<p class="text-amber-200" role="status">Couldn't read {hintsFailure.file}: {hintsFailure.reason}</p>{/if}
		{#if hintsNotes}
			{#each hintsNotes.parents as p (p)}<p class="text-amber-200">Parent not found: {p}</p>{/each}
			{#each hintsNotes.leads as l (l)}<p class="text-amber-200">Lead not found: {l}</p>{/each}
			{#each hintsNotes.members as m (m)}<p class="text-amber-200">Not found in this import: {m}</p>{/each}
			{#each hintsNotes.circles as c (c)}<p class="text-amber-200">{c} owns itself through other teams: it goes at the top.</p>{/each}
		{/if}
		{#if editingTeams}
			<ul class="space-y-2" aria-label="Teams to create">
				{#each drafts as d (d.key)}
					<li class="grid gap-2 rounded-md bg-gray-800/60 p-2 sm:grid-cols-[1fr_1fr_1fr_auto]" data-testid="team-edit" data-team={d.key}>
						<Input aria-label="Name of {d.name}" value={d.name} onchange={(e) => rename(d, e.currentTarget.value)} disabled={busy} />
						<select aria-label="Owner of {d.name}" class={select} value={d.parentKey ?? ''} disabled={busy} onchange={(e) => ((drafts = setParent(drafts, d.key, e.currentTarget.value || undefined)), refresh())}>
							<option value="">Top level</option>
							{#each drafts.filter((x) => x.key !== d.key) as x (x.key)}<option value={x.key}>{x.name}</option>{/each}
						</select>
						<select aria-label="Lead of {d.name}" class={select} value={d.leadKey ?? ''} disabled={busy} onchange={(e) => setLead(d, e.currentTarget.value)}>
							<option value="">No lead</option>
							{#each leadChoices(d) as m (m.key)}<option value={m.key}>{m.name}</option>{/each}
						</select>
						<Button type="button" variant="outline" size="sm" disabled={busy} onclick={() => remove(d)} aria-label="Remove {d.name}">Remove</Button>
					</li>
				{/each}
			</ul>
			<form class="flex items-end gap-2" onsubmit={(e) => (e.preventDefault(), add())}>
				<div class="grid gap-1">
					<Label for="new-team-name">New team</Label>
					<Input id="new-team-name" bind:value={newTeamName} disabled={busy} autocomplete="off" />
				</div>
				<Button type="submit" variant="outline" size="sm" disabled={busy || newTeamName.trim() === ''}>Add team</Button>
			</form>
		{/if}
	</section>

	{#if project}
		<section class="space-y-2 rounded-lg bg-white/5 p-3 text-sm/6 outline-1 -outline-offset-1 outline-white/10" aria-label="Project">
			<label class="flex items-center gap-x-2 text-white">
				<input type="checkbox" bind:checked={projectOn} disabled={busy} />
				Project · key <span class="font-mono">{project.key}</span>, shared by {project.teamKeys.map(teamName).join(', ') || 'no team yet'}
			</label>
			{#if projectOn}
				<div class="grid max-w-sm gap-1">
					<Label for="project-name">Name it</Label>
					<Input id="project-name" value={project.name} onchange={(e) => (project = { ...project!, name: e.currentTarget.value.trim() || project!.name })} disabled={busy} autocomplete="off" />
				</div>
			{/if}
		</section>
	{/if}
</div>
