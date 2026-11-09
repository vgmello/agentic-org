// The effective set of processes for one piece of work, as design §3.2 orders it (`org_process::resolve`, pure):
//   1. valid files only            2. a key used twice at one level and scope leaves both out
//   3. a seal refuses lower files   4. fold each team chain, the nearer team winning
//   5. combine the chains (a conflict when two teams disagree)   6. the project wins, and settles a conflict
//   7. a surviving "off" removes the key   8. private files only add   9. drift of an override
// The result keeps each key's chain of layers, which is what "why" shows.

import type { Drift, EffectiveSet, Entry, EntryKind, Layer, Level, Problem, ProcessDoc, ResolveInput, TeamRef, TurnedOff } from './types';
import { LEVEL_LABEL } from './types';

const sameScope = (a: string | undefined, b: string | undefined) => (a ?? '') === (b ?? '');

/** A file whose front matter is unreadable, or whose level and scope differ from where it lives, or that is a draft or retired. */
function classify(doc: ProcessDoc): 'valid' | 'invalid' | 'mismatch' | 'inactive' {
	if (doc.problems && doc.problems.length > 0) return 'invalid';
	if (doc.level !== doc.location.level || !sameScope(doc.scope, doc.location.scope)) return 'mismatch';
	return doc.status === 'active' || doc.status === 'off' ? 'valid' : 'inactive';
}

/** A key that stays sealed even when its file is left out: a seal fails closed (a file too broken to tell counts as sealed). */
const failsClosed = (doc: ProcessDoc, kind: ReturnType<typeof classify>) =>
	(kind === 'invalid' || kind === 'mismatch') && (!doc.overridable || (doc.problems ?? []).some((p) => p.field === 'overridable'));

export function resolve(input: ResolveInput): EffectiveSet {
	const { docs, teams, project, agentId } = input;
	const set: EffectiveSet = { entries: [], turnedOff: [], conflicts: [], problems: [] };
	const teamById = new Map<string, TeamRef>(teams.map((t) => [t.id, t]));
	const scopeName = (level: Level, scope: string | undefined): string | undefined => (level === 'team' ? teamById.get(scope ?? '')?.name : level === 'project' ? project?.name : level === 'agent' ? scope : undefined);

	// The team chains of the project: each linked team from its outermost parent down; a linked team that is an ancestor of
	// another linked team is dropped (its chain is part of the other's).
	const chainOf = (teamId: string): TeamRef[] => {
		const chain: TeamRef[] = [];
		for (let at = teamById.get(teamId), hops = 0; at && hops <= teams.length; at = at.owner_team_id ? teamById.get(at.owner_team_id) : undefined, hops++) chain.unshift(at);
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
		if (kind === 'invalid') {
			problem('invalid', doc, `Details not recognized: ${doc.problems!.map((p) => p.field).join(', ')}.`);
		} else if (kind === 'mismatch') {
			problem('mismatch', doc, "Details not recognized: its level or scope doesn't match where the file lives.");
		}
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

	// Seals: org files, and team files of each chain (they bind that team, its sub-teams and their projects).
	const sealingDocs = [...candidates.filter((d) => kinds.get(d) === 'valid' ? !d.overridable && d.status === 'active' : failsClosed(d, kinds.get(d)!)), ...[...clashing].filter((d) => !d.overridable && d.status === 'active')];
	const orgSeal = (key: string) => sealingDocs.find((d) => d.location.level === 'organization' && d.key === key);
	const teamSeal = (key: string, teamId: string) => sealingDocs.find((d) => d.location.level === 'team' && d.location.scope === teamId && d.key === key);

	// 3 and 4. Each team chain, outermost first: a file is refused if the organization or a team above it sealed its key; the nearer wins.
	const refused = new Map<ProcessDoc, string>();
	const refuse = (doc: ProcessDoc, by: ProcessDoc) => {
		const who = by.location.level === 'organization' ? 'the organization' : (scopeName('team', by.location.scope) ?? 'a team');
		refused.set(doc, `${doc.name} is sealed by ${who}. You can add a process instead.`);
		problem('sealed_refused', doc, `${doc.name} is sealed by ${who}. You can add a process instead.`);
	};
	const chainWinners = chains.map((chain) => {
		const winners = new Map<string, ProcessDoc>();
		chain.forEach((team, index) => {
			for (const doc of live.filter((d) => d.location.level === 'team' && d.location.scope === team.id)) {
				const above = orgSeal(doc.key) ?? chain.slice(0, index).map((t) => teamSeal(doc.key, t.id)).find(Boolean);
				if (above && above !== doc) {
					if (!refused.has(doc)) refuse(doc, above);
					continue;
				}
				winners.set(doc.key, doc);
			}
		});
		return winners;
	});

	// 5. Combine the chains. One distinct winner applies; two or more are a conflict.
	const orgDocs = new Map(live.filter((d) => d.location.level === 'organization').map((d) => [d.key, d]));
	const teamResult = new Map<string, ProcessDoc>();
	const conflictKeys = new Set<string>();
	for (const key of new Set(chainWinners.flatMap((w) => [...w.keys()]))) {
		const winners = [...new Map(chainWinners.flatMap((w) => (w.has(key) ? [[w.get(key)!.id, w.get(key)!] as const] : []))).values()];
		if (winners.length === 1) {
			teamResult.set(key, winners[0]);
			continue;
		}
		conflictKeys.add(key);
		const teamsInvolved = winners.map((d) => scopeName('team', d.location.scope) ?? 'a team');
		const sealed = winners.filter((d) => !d.overridable && d.status === 'active');
		const org = orgDocs.get(key);
		const byTeamId = (a: ProcessDoc, b: ProcessDoc) => (a.location.scope ?? '').localeCompare(b.location.scope ?? '');
		if (sealed.length > 0) {
			// Sealed by a team: it applies meanwhile (two different seals: the organization's version, else the first by team id that is not an off).
			const meanwhile = sealed.length === 1 ? sealed[0] : (org ?? [...winners].sort(byTeamId).find((d) => d.status !== 'off'));
			if (meanwhile) teamResult.set(key, meanwhile);
			set.conflicts.push({ key, teams: teamsInvolved, applies: `Sealed by ${scopeName('team', sealed[0].location.scope)}; applies until the project decides`, ...(meanwhile ? { doc: meanwhile } : {}) });
		} else {
			set.conflicts.push({ key, teams: teamsInvolved, applies: org ? "The organization's version applies until the project decides" : 'Neither team’s version applies until the project decides', ...(org ? { doc: org } : {}) });
		}
	}
	const teamSealOnProject = (key: string) => chains.some((chain) => chain.some((t) => teamSeal(key, t.id)));

	// 6. The project's own file wins, unless the organization sealed the key, or a team seal binds it and no conflict has opened it up.
	const projectDocs = new Map<string, ProcessDoc>();
	for (const doc of live.filter((d) => d.location.level === 'project')) {
		const above = orgSeal(doc.key) ?? (teamSealOnProject(doc.key) && !conflictKeys.has(doc.key) ? chains.flatMap((c) => c.map((t) => teamSeal(doc.key, t.id))).find(Boolean) : undefined);
		if (above && above !== doc) {
			refuse(doc, above);
			continue;
		}
		projectDocs.set(doc.key, doc);
	}

	// The project's own file settles any conflict on its key: it is not a conflict any more.
	set.conflicts = set.conflicts.filter((c) => !projectDocs.has(c.key));

	// What each key resolves to, and 7. a surviving "off" removes it.
	const keys = new Set([...orgDocs.keys(), ...teamResult.keys(), ...projectDocs.keys()]);
	const winners = new Map<string, ProcessDoc>();
	for (const key of keys) {
		const win = projectDocs.get(key) ?? teamResult.get(key) ?? orgDocs.get(key);
		if (win) winners.set(key, win);
	}
	const higherThan = (doc: ProcessDoc): ProcessDoc | undefined => {
		const order: Level[] = ['organization', 'team', 'project', 'agent'];
		const mine = order.indexOf(doc.location.level);
		const above = [orgDocs.get(doc.key), ...chainWinners.map((w) => w.get(doc.key)), projectDocs.get(doc.key)].filter((d): d is ProcessDoc => !!d && order.indexOf(d.location.level) < mine);
		return above.sort((a, b) => order.indexOf(b.location.level) - order.indexOf(a.location.level))[0];
	};
	const layersOf = (key: string, win: ProcessDoc): Layer[] => {
		const order: Level[] = ['organization', 'team', 'project', 'agent'];
		return candidates
			.filter((d) => d.key === key)
			.sort((a, b) => order.indexOf(a.location.level) - order.indexOf(b.location.level))
			.map((doc): Layer => {
				const kind = kinds.get(doc)!;
				const name = scopeName(doc.location.level, doc.location.scope);
				const base = { level: doc.location.level, ...(name ? { scopeName: name } : {}), doc };
				if (kind === 'invalid' || kind === 'mismatch' || clashing.has(doc)) return { ...base, outcome: 'left_out', note: kind === 'valid' ? 'Left out: another file uses the same key here.' : 'Left out: its details are not recognized.' };
				if (refused.has(doc)) return { ...base, outcome: 'refused', note: refused.get(doc) };
				if (doc === win) return { ...base, outcome: doc.status === 'off' ? 'turned_off' : 'applies', ...(doc.status === 'off' ? { note: `Turned off: ${doc.reason ?? ''}` } : {}) };
				return { ...base, outcome: 'overridden', note: `Overridden by ${LEVEL_LABEL[win.location.level].toLowerCase()}${scopeName(win.location.level, win.location.scope) ? ` ${scopeName(win.location.level, win.location.scope)}` : ''}.` };
			});
	};
	const driftOf = (doc: ProcessDoc): Drift | undefined => {
		const parent = higherThan(doc);
		if (!doc.overrides || !parent || doc.overrides.version >= parent.version) return undefined;
		return { wrote: doc.overrides.version, parentVersion: parent.version, parentLevel: parent.location.level, parentName: scopeName(parent.location.level, parent.location.scope) ?? LEVEL_LABEL[parent.location.level] };
	};
	const entryOf = (key: string, doc: ProcessDoc): Entry => {
		const hasHigher = !!higherThan(doc);
		const kind: EntryKind = hasHigher ? 'override' : doc.location.level === 'organization' ? 'inherited' : 'addition';
		const conflict = set.conflicts.find((c) => c.key === key);
		const drift = driftOf(doc);
		return {
			key,
			doc,
			kind,
			source: doc.location.level,
			...(scopeName(doc.location.level, doc.location.scope) ? { scopeName: scopeName(doc.location.level, doc.location.scope) } : {}),
			sealed: !doc.overridable,
			...(conflict && conflict.doc === doc && !doc.overridable ? { sealedNote: conflict.applies } : {}),
			...(drift ? { drift } : {}),
			chain: layersOf(key, doc)
		};
	};
	const turnedOff: TurnedOff[] = [];
	const entries: Entry[] = [];
	for (const [key, doc] of winners) {
		if (doc.status === 'off') {
			turnedOff.push({ key, name: doc.name, level: doc.location.level, ...(scopeName(doc.location.level, doc.location.scope) ? { scopeName: scopeName(doc.location.level, doc.location.scope) } : {}), reason: doc.reason ?? '', doc });
			continue;
		}
		entries.push(entryOf(key, doc));
	}

	// 8. Private files only add: not applied, and flagged, if they override or turn off, or the key is used at an inherited level of this project.
	if (agentId) {
		const inherited = new Set(candidates.filter((d) => d.location.level !== 'agent').map((d) => d.key));
		for (const doc of live.filter((d) => d.location.level === 'agent')) {
			if (doc.overrides) problem('private_only_adds', doc, "Private processes only add: they can't override a process.");
			else if (doc.status === 'off') problem('private_only_adds', doc, "Private processes only add: they can't turn off a process.");
			else if (inherited.has(doc.key)) problem('private_only_adds', doc, `Private processes only add: ${doc.name} isn't applied because it changes an inherited process.`);
			else entries.push({ key: doc.key, doc, kind: 'addition', source: 'agent', scopeName: agentId, sealed: false, chain: layersOf(doc.key, doc) });
		}
	}

	set.entries = entries.sort((a, b) => a.doc.name.localeCompare(b.doc.name));
	set.turnedOff = turnedOff.sort((a, b) => a.name.localeCompare(b.name));
	return set;
}
