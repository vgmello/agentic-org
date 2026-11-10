// The effective set of processes for one piece of work, as design §3.2 orders it (`org_process::resolve`, pure):
//   1. valid files only            2. a key used twice at one level and scope leaves both out
//   3. a seal refuses lower files   4. fold each team chain, the nearer team winning
//   5. combine the chains (a conflict when two teams disagree)   6. the project wins, and settles a conflict
//   7. a surviving "off" removes the key   8. private files only add   9. drift of an override
// The result keeps each key's chain of layers, which is what "why" shows.
//
// A draft or retired file is simply left out: work in progress is not a problem to report.

import type { Drift, EffectiveSet, Entry, EntryKind, Layer, Level, Problem, ProcessDoc, ResolveInput, TeamRef, TurnedOff } from './types';
import { BRANCH_KEY, cannotApprove } from './rules';
import { LEVEL_LABEL } from './types';

const STATUSES: string[] = ['draft', 'active', 'off', 'retired'];
const LEVELS: Level[] = ['organization', 'team', 'project', 'agent'];
const sameScope = (a: string | undefined, b: string | undefined) => (a ?? '') === (b ?? '');
/** A plain code-unit order: the same on every machine. */
const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** A file whose front matter is unreadable, or whose level and scope differ from where it lives, or that is a draft or retired. */
function classify(doc: ProcessDoc): 'valid' | 'invalid' | 'mismatch' | 'inactive' {
	if ((doc.problems && doc.problems.length > 0) || !STATUSES.includes(doc.status)) return 'invalid';
	if (doc.level !== doc.location.level || !sameScope(doc.scope, doc.location.scope)) return 'mismatch';
	return doc.status === 'active' || doc.status === 'off' ? 'valid' : 'inactive';
}

/** A key that stays sealed even when its file is left out: a seal fails closed (a file too broken to tell counts as sealed). Drafts and retired files never seal. */
const failsClosed = (doc: ProcessDoc, kind: ReturnType<typeof classify>) =>
	(kind === 'invalid' || kind === 'mismatch') && doc.status !== 'draft' && doc.status !== 'retired' && (!doc.overridable || (doc.problems ?? []).some((p) => p.field === 'overridable'));

export function resolve(input: ResolveInput): EffectiveSet {
	const { docs, teams, project, agentId } = input;
	const set: EffectiveSet = { entries: [], turnedOff: [], conflicts: [], problems: [] };
	const teamById = new Map<string, TeamRef>(teams.map((t) => [t.id, t]));
	const scopeName = (level: Level, scope: string | undefined): string | undefined => (level === 'team' ? teamById.get(scope ?? '')?.name : level === 'project' ? project?.name : level === 'agent' ? scope : undefined);
	const whereOf = (doc: ProcessDoc) => scopeName(doc.location.level, doc.location.scope);

	// The team chains of the project: each linked team from its outermost parent down (a parent loop is cut where it repeats); a
	// linked team that is an ancestor of another linked team is dropped (its chain is part of the other's).
	const chainOf = (teamId: string): TeamRef[] => {
		const chain: TeamRef[] = [];
		const seen = new Set<string>();
		for (let at = teamById.get(teamId); at && !seen.has(at.id); at = at.owner_team_id ? teamById.get(at.owner_team_id) : undefined) {
			seen.add(at.id);
			chain.unshift(at);
		}
		return chain;
	};
	const ancestors = new Set((project?.teamIds ?? []).flatMap((id) => chainOf(id).slice(0, -1).map((t) => t.id)));
	const chains = (project?.teamIds ?? []).filter((id) => !ancestors.has(id) && teamById.has(id)).map(chainOf);
	const chainTeamIds = new Set(chains.flatMap((c) => c.map((t) => t.id)));

	// The files that can matter to this set, by where they live.
	const here = (doc: ProcessDoc): boolean => {
		const { level, scope } = doc.location;
		if (level === 'organization') return true;
		if (level === 'team') return chainTeamIds.has(scope ?? '');
		if (level === 'project') return !!project && scope === project.key;
		return !!agentId && scope === agentId;
	};
	const candidates = docs.filter(here);
	const kinds = new Map(candidates.map((d) => [d, classify(d)]));
	const problem = (kind: Problem['kind'], doc: ProcessDoc, text: string, other?: ProcessDoc) => set.problems.push({ kind, key: doc.key, doc, text, ...(other ? { other } : {}) });

	// 1. Valid files only. The others are listed with their problem, and a sealed one still seals its key.
	for (const doc of candidates) {
		const kind = kinds.get(doc)!;
		if (kind === 'invalid') problem('invalid', doc, `Details not recognized (${(doc.problems?.length ? doc.problems.map((p) => p.field) : ['status']).join(', ')}).`);
		else if (kind === 'mismatch') problem('mismatch', doc, "Details not recognized (level or scope): they don't match where the file lives.");
	}
	const valid = candidates.filter((d) => kinds.get(d) === 'valid');

	// 2. A key used twice at one level and scope: both are left out (and a sealed one still seals).
	const clashing = new Set<ProcessDoc>();
	const groups = new Map<string, ProcessDoc[]>();
	for (const doc of valid) {
		const id = `${doc.location.level}|${doc.location.scope ?? ''}|${doc.key}`;
		groups.set(id, [...(groups.get(id) ?? []), doc]);
	}
	for (const group of groups.values()) {
		if (group.length < 2) continue;
		const where = scopeName(group[0].location.level, group[0].location.scope);
		for (const doc of group) {
			clashing.add(doc);
			problem('clash', doc, `Two processes use the key \`${doc.key}\` in ${where ?? LEVEL_LABEL[doc.location.level].toLowerCase()}. Neither applies until one is renamed.`, group.find((d) => d !== doc));
		}
	}
	const live = valid.filter((d) => !clashing.has(d));

	// Seals: org files, and team files of each chain (they bind that team, its sub-teams and their projects). A sealed file that is
	// left out (too broken to read, in a clash) still seals.
	// Any sealed file that is not a draft or retired seals, a turning-off included (a sealed process can't be overridden or turned off).
	const sealing = [...live.filter((d) => !d.overridable), ...candidates.filter((d) => failsClosed(d, kinds.get(d)!)), ...[...clashing].filter((d) => !d.overridable)];
	const orgSeal = (key: string) => sealing.find((d) => d.location.level === 'organization' && d.key === key);
	const teamSeal = (key: string, teamId: string) => sealing.find((d) => d.location.level === 'team' && d.location.scope === teamId && d.key === key);

	// 3 and 4. Each team chain, outermost first: a file is refused if the organization or a team above it sealed its key; the nearer wins.
	const refused = new Map<ProcessDoc, string>();
	const refuse = (doc: ProcessDoc, by: ProcessDoc) => {
		const who = by.location.level === 'organization' ? 'the organization' : (scopeName('team', by.location.scope) ?? 'a team');
		// Said once, naming who tried: "Platform's version of Security basics isn't applied: the organization sealed it."
		const text = `${whereOf(doc) ? `${whereOf(doc)}'s` : 'Its'} version of ${by.name} isn't applied: ${who} sealed it. You can add a process instead.`;
		refused.set(doc, text);
		problem('sealed_refused', doc, text, by);
	};
	const orgDocs = new Map(live.filter((d) => d.location.level === 'organization').map((d) => [d.key, d]));
	/** The file that applies for a key from the layers above `doc`: its parent, for kind and drift. */
	const chainParent = new Map<ProcessDoc, ProcessDoc | undefined>();
	const chainIndex = new Map<ProcessDoc, number>();
	const chainWinners = chains.map((chain) => {
		const winners = new Map<string, ProcessDoc>();
		chain.forEach((team, index) => {
			for (const doc of live.filter((d) => d.location.level === 'team' && d.location.scope === team.id)) {
				const above = orgSeal(doc.key) ?? chain.slice(0, index).map((t) => teamSeal(doc.key, t.id)).find(Boolean);
				if (above && above !== doc) {
					if (!refused.has(doc)) refuse(doc, above);
					continue;
				}
				// Its parent: the nearest file above it in its own chain, else the organization's.
				chainParent.set(doc, winners.get(doc.key) ?? orgDocs.get(doc.key));
				chainIndex.set(doc, index);
				winners.set(doc.key, doc);
			}
		});
		return winners;
	});

	// 5. Combine the chains. One distinct winner applies; two or more are a conflict. A chain whose nearest file for a key is a seal
	// that could not be read (left out) counts as a sealed winner that cannot apply: a sibling's file never silently cancels it.
	const deadSeal = (chain: TeamRef[], key: string): ProcessDoc | undefined => [...chain].reverse().map((t) => teamSeal(key, t.id)).find((d) => !!d && !live.includes(d));
	const teamResult = new Map<string, ProcessDoc>();
	const conflictKeys = new Set<string>();
	const allKeys = new Set([...chainWinners.flatMap((w) => [...w.keys()]), ...chains.flatMap((c) => [...new Set(sealing.filter((d) => d.location.level === 'team' && c.some((t) => t.id === d.location.scope)).map((d) => d.key))])]);
	for (const key of [...allKeys].sort(cmp)) {
		const winners = [...new Set(chainWinners.flatMap((w) => (w.has(key) ? [w.get(key)!] : [])))].sort((a, b) => cmp(a.location.scope ?? '', b.location.scope ?? '') || cmp(a.id, b.id));
		// A broken seal counts only in a chain with no valid file for the key (next to a valid one it is a clash, left out).
		const dead = chains.flatMap((c, i) => (chainWinners[i].has(key) ? [] : [deadSeal(c, key)])).filter((d): d is ProcessDoc => !!d);
		if (winners.length === 1 && dead.length === 0) {
			teamResult.set(key, winners[0]);
			continue;
		}
		if (winners.length === 0) continue; // only an unreadable seal: nothing from the teams applies, and the key stays sealed below
		conflictKeys.add(key);
		const teamsInvolved = [...new Set([...winners, ...dead].sort((a, b) => cmp(a.location.scope ?? '', b.location.scope ?? '') || cmp(a.id, b.id)).map((d) => scopeName('team', d.location.scope) ?? 'a team'))];
		const sealed = winners.filter((d) => !d.overridable);
		const org = orgDocs.get(key);
		const byTeamId = (a: ProcessDoc, b: ProcessDoc) => cmp(a.location.scope ?? '', b.location.scope ?? '') || cmp(a.id, b.id);
		let meanwhile: ProcessDoc | undefined;
		if (sealed.length > 0 || dead.length > 0) {
			// Sealed by a team: it applies meanwhile (two different seals: the organization's version, else the first by team id that is not an off).
			const readable = [...sealed].sort(byTeamId);
			const only = dead.length === 0 && readable.length === 1 ? readable[0] : undefined;
			meanwhile = only ?? org ?? readable.find((d) => d.status !== 'off') ?? readable[0];
		} else meanwhile = org;
		if (meanwhile && meanwhile.location.level === 'team') teamResult.set(key, meanwhile);
		const sealer = meanwhile?.location.level === 'team' ? meanwhile : (sealed[0] ?? dead[0]);
		const applies = !meanwhile
			? 'Neither team’s version applies until the project decides'
			: meanwhile.location.level === 'organization'
				? "The organization's version applies until the project decides"
				: `Sealed by ${scopeName('team', (sealer ?? meanwhile).location.scope)}; applies until the project decides`;
		set.conflicts.push({ key, name: (org ?? winners[0] ?? dead[0]).name, teams: teamsInvolved, applies, ...(meanwhile ? { doc: meanwhile } : {}) });
	}
	const teamSealOnProject = (key: string) => chains.flatMap((chain) => chain.map((t) => teamSeal(key, t.id))).find(Boolean);

	// 6. The project's own file wins, unless the organization sealed the key, or a team seal binds it and no conflict has opened it up.
	const projectDocs = new Map<string, ProcessDoc>();
	for (const doc of live.filter((d) => d.location.level === 'project')) {
		const above = orgSeal(doc.key) ?? (conflictKeys.has(doc.key) ? undefined : teamSealOnProject(doc.key));
		if (above && above !== doc) {
			refuse(doc, above);
			continue;
		}
		projectDocs.set(doc.key, doc);
	}
	// The project's own file settles any conflict on its key: it is not a conflict any more. Its parent is what applied until then.
	const conflicts = set.conflicts;
	for (const [key, doc] of projectDocs) chainParent.set(doc, conflicts.find((c) => c.key === key)?.doc ?? teamResult.get(key) ?? orgDocs.get(key));
	set.conflicts = conflicts; // filtered below, once the approval of a turning-off is known

	// What each key resolves to, and 7. a surviving "off" removes it.
	// A turning-off of the one open branch rule's process counts only when someone other than its author, and an approver of the level,
	// approved it: otherwise it is not in force (the process stays, and the rule stays on).
	const unapprovedOff = new Map<ProcessDoc, string>();
	// Without the approval context nobody is known to be an approver: the turning-off does not count (the rule fails safe).
	const approvedOff = (doc: ProcessDoc) => !!doc.author && !!doc.approvedBy && doc.approvedBy !== doc.author && !!input.approval && cannotApprove(doc.location, doc.author, doc.approvedBy, input.approval) === undefined;
	const winners = new Map<string, ProcessDoc>();
	for (const key of new Set([...orgDocs.keys(), ...teamResult.keys(), ...projectDocs.keys()])) {
		let win = projectDocs.get(key) ?? teamResult.get(key) ?? orgDocs.get(key);
		while (win && win.key === BRANCH_KEY && win.status === 'off' && !approvedOff(win)) {
			unapprovedOff.set(win, `Turning it off needs approval by someone other than its author (${win.author ?? 'unknown'}).`);
			win = chainParent.get(win);
		}
		if (win) winners.set(key, win);
	}
	// The project's own file settles a conflict on its key, unless it is a turning-off that does not count.
	set.conflicts = set.conflicts.filter((c) => !projectDocs.has(c.key) || unapprovedOff.has(projectDocs.get(c.key)!));

	// 8. Private files only add (their problems come first, so a chain can say why a private file is not applied).
	const privateProblem = new Map<ProcessDoc, string>();
	if (agentId) {
		const inherited = new Set(candidates.filter((d) => d.location.level !== 'agent').map((d) => d.key));
		for (const doc of live.filter((d) => d.location.level === 'agent')) {
			const text = doc.overrides ? `Private processes only add: ${doc.name} can't override a process.` : doc.status === 'off' ? `Private processes only add: ${doc.name} can't turn off a process.` : inherited.has(doc.key) ? `Private processes only add: ${doc.name} isn't applied because it changes an inherited process.` : undefined;
			if (text) {
				privateProblem.set(doc, text);
				problem('private_only_adds', doc, text);
			}
		}
	}

	const layersOf = (key: string, win: ProcessDoc | undefined): Layer[] =>
		candidates
			.filter((d) => d.key === key)
			.sort((a, b) => LEVELS.indexOf(a.location.level) - LEVELS.indexOf(b.location.level) || (chainIndex.get(a) ?? 0) - (chainIndex.get(b) ?? 0) || cmp(a.location.scope ?? '', b.location.scope ?? '') || cmp(a.id, b.id))
			.map((doc): Layer => {
				const kind = kinds.get(doc)!;
				const name = whereOf(doc);
				const base = { level: doc.location.level, ...(name ? { scopeName: name } : {}), doc };
				if (kind === 'inactive') return { ...base, outcome: 'left_out', note: `Left out: it is a ${doc.status}.` };
				if (kind !== 'valid') return { ...base, outcome: 'left_out', note: 'Left out: its details are not recognized.' };
				if (clashing.has(doc)) return { ...base, outcome: 'left_out', note: 'Left out: another file uses the same key here.' };
				if (refused.has(doc)) return { ...base, outcome: 'refused', note: refused.get(doc) };
				if (privateProblem.has(doc)) return { ...base, outcome: 'not_applied', note: privateProblem.get(doc) };
				if (unapprovedOff.has(doc)) return { ...base, outcome: 'not_applied', note: unapprovedOff.get(doc) };
				if (doc === win) return doc.status === 'off' ? { ...base, outcome: 'turned_off', note: `Turned off: ${doc.reason ?? ''}` } : { ...base, outcome: 'applies' };
				const conflict = set.conflicts.find((c) => c.key === key);
				if (conflict && doc.location.level === 'team') return { ...base, outcome: 'conflict', note: `${name ?? 'This team'}'s version conflicts with another team's. ${conflict.applies}.` };
				return { ...base, outcome: 'overridden', ...(win ? { note: `Overridden by ${LEVEL_LABEL[win.location.level].toLowerCase()}${whereOf(win) ? ` ${whereOf(win)}` : ''}.` } : {}) };
			});

	const entryOf = (key: string, doc: ProcessDoc): Entry => {
		const parent = chainParent.get(doc);
		const kind: EntryKind = parent ? 'override' : doc.location.level === 'organization' ? 'inherited' : 'addition';
		const conflict = set.conflicts.find((c) => c.key === key);
		let drift: Drift | undefined;
		let driftUnknown = false;
		if (kind === 'override') {
			// An override says which version of which key it was written against; without that, whether its parent changed is unknown.
			if (!doc.overrides || doc.overrides.key !== doc.key) driftUnknown = true;
			else if (parent && doc.overrides.version < parent.version) drift = { wrote: doc.overrides.version, parentVersion: parent.version, parentLevel: parent.location.level, parentName: whereOf(parent) ?? LEVEL_LABEL[parent.location.level] };
		}
		const name = whereOf(doc);
		return {
			key,
			doc,
			kind,
			source: doc.location.level,
			...(name ? { scopeName: name } : {}),
			sealed: !doc.overridable,
			...(conflict && conflict.doc === doc && !doc.overridable ? { sealedNote: conflict.applies } : {}),
			...(drift ? { drift } : {}),
			...(driftUnknown ? { driftUnknown } : {}),
			...(chainIndex.has(doc) ? { depth: chainIndex.get(doc) } : {}),
			chain: layersOf(key, doc)
		};
	};
	const turnedOff: TurnedOff[] = [];
	const entries: Entry[] = [];
	for (const [key, doc] of winners) {
		if (doc.status === 'off') {
			const name = whereOf(doc);
			turnedOff.push({ key, chain: layersOf(key, doc), name: doc.name, level: doc.location.level, ...(name ? { scopeName: name } : {}), reason: doc.reason ?? '', doc });
		} else entries.push(entryOf(key, doc));
	}
	for (const doc of live.filter((d) => d.location.level === 'agent' && !privateProblem.has(d))) entries.push({ key: doc.key, doc, kind: 'addition', source: 'agent', ...(agentId ? { scopeName: agentId } : {}), sealed: false, chain: layersOf(doc.key, doc) });

	const byName = (a: { name: string; key: string }, b: { name: string; key: string }) => cmp(a.name.toLowerCase(), b.name.toLowerCase()) || cmp(a.key, b.key);
	set.conflicts.sort((a, b) => cmp(a.key, b.key));
	set.problems.sort((a, b) => cmp(a.key, b.key) || LEVELS.indexOf(a.doc.location.level) - LEVELS.indexOf(b.doc.location.level) || cmp(a.doc.location.scope ?? '', b.doc.location.scope ?? '') || cmp(a.doc.id, b.doc.id) || cmp(a.kind, b.kind));
	set.entries = entries.sort((a, b) => byName({ name: a.doc.name, key: a.key }, { name: b.doc.name, key: b.key }));
	set.turnedOff = turnedOff.sort((a, b) => byName(a, b));
	return set;
}
