// A sample conversation history for the mock office: invented, none from a real workspace. It is built on the sample
// organization (src/lib/org/sample.ts), and any message whose people aren't in the organization is left out, so the
// organization that has only its owner gets #general and nothing else.
import { APPROVAL_OPTIONS, emptyChat, OFFICE, syncAuto, type ChatContext, type ChatMember } from './engine';
import type { ChatMessage, ChatState, Conversation, Presence } from './types';

const MIN = 60_000;

/**
 * Members the sample chat knows that the sample organization doesn't list: the built-in HR lead, and Marlowe, an agent that
 * was offboarded and is kept as a record of its conversations.
 */
export const SAMPLE_DIRECTORY: ChatMember[] = [
	{ id: 'hr', kind: 'agent', name: 'HR lead', handle: 'hr' },
	{ id: 'marlowe', kind: 'agent', name: 'Marlowe', handle: 'marlowe', former: true }
];

export function sampleChat(ctx: ChatContext): ChatState {
	const at = (minutesAgo: number) => new Date(Date.parse(ctx.now) - minutesAgo * MIN).toISOString();
	const has = (id: string) => ctx.members.some((m) => m.id === id);
	let state = syncAuto(emptyChat(), { ...ctx, newId: undefined });
	const owner = ctx.owner;
	const conversations: Conversation[] = [];
	const dm = (id: string, members: string[], kind: Conversation['kind'] = 'dm') => {
		if (members.every(has)) conversations.push({ id, kind, members, created_at: at(60 * 48) });
	};
	if (['sam', 'noor', 'avery'].every(has)) conversations.push({ id: 'c-release-v1', kind: 'channel', name: 'release-v1', purpose: 'Shipping v1: dates, blockers and the go/no-go.', access: 'open', members: [owner, 'sam', 'kai', 'avery'], created_at: at(60 * 30) });
	if (['juno', 'pax', 'noor'].every(has)) conversations.push({ id: 'c-design-crit', kind: 'channel', name: 'design-crit', purpose: 'Weekly critique of Beacon and Atlas screens.', access: 'open', members: ['juno', 'pax', 'noor'], created_at: at(60 * 30) });
	dm('c-dm-sam', [owner, 'sam']);
	dm('c-dm-ines', [owner, 'ines']);
	dm('c-dm-avery', [owner, 'avery']);
	dm('c-dm-pax', [owner, 'pax']);
	dm('c-group', [owner, 'sam', 'noor'], 'group_dm');
	dm('c-dm-sam-noor', ['sam', 'noor']);
	state = { ...state, conversations: [...state.conversations, ...conversations] };

	const general = state.conversations.find((c) => c.auto?.type === 'general')!;
	const channelOf = (projectId: string) => state.conversations.find((c) => c.auto?.type === 'project' && c.auto.project_id === projectId)?.id;
	const platform = state.conversations.find((c) => c.auto?.type === 'team' && c.auto.team_id === 'platform')?.id;
	const atlas = channelOf('atlas');

	const messages: ChatMessage[] = [];
	let n = 0;
	const add = (conversation_id: string | undefined, from: string, kind: ChatMessage['kind'], minutesAgo: number, body: string, extra: Partial<ChatMessage> = {}): string => {
		const id = `m-${++n}`;
		if (conversation_id && (from === OFFICE || has(from)) && (extra.addressed_to === undefined || has(extra.addressed_to))) messages.push({ id, conversation_id, from, kind, body, created_at: at(minutesAgo), ...extra });
		return id;
	};

	// #general
	add(general.id, 'avery', 'msg', 60 * 5, ['Plan for the week:', ...Array.from({ length: 19 }, (_, i) => `${i + 1}. Step ${i + 1}: keep the notes in the shared doc and link them in the task.`), 'Questions in the thread, please.'].join('\n'));
	const ready = add(general.id, 'sam', 'announcement', 50, 'Ready for review: **create-org** branch at `9c1e0b2`. See ac-T-17 and [the branch](https://example.com/branch/create-org).', { reason: 'ready_for_review', reactions: { seen: ['avery'] } });
	add(general.id, 'sam', 'announcement', 47, 'Tests are green on create-org.', { reason: 'report', thread_root_id: ready, folded: true });
	add(general.id, 'sam', 'announcement', 43, 'Docs for create-org are updated.', { reason: 'report', thread_root_id: ready, folded: true });
	add(general.id, 'sam', 'announcement', 39, 'Merged the review fixes.', { reason: 'report', thread_root_id: ready, folded: true });
	add(general.id, 'juno', 'announcement', 20, '@owner the Beacon mockups are ready for your go.', { reason: 'decision_needed' });

	// #atlas (the Atlas project): Kai and Mira are on its team
	if (atlas) {
		add(atlas, 'kai', 'request', 12, 'Block bug reports when a project has no team?', {
			addressed_to: owner,
			options: [
				{ id: 'yes', label: 'Yes, block' },
				{ id: 'no', label: 'No, allow (default)' }
			],
			default_option: 'no',
			default_at: at(-120),
			wait_until: at(-120),
			blocks: 'T-A42'
		});
		add(atlas, 'mira', 'request', 9, 'Publish the 0.2 release notes? They list T-A31 and ADR-0004.', { addressed_to: owner, approval: true, options: APPROVAL_OPTIONS, blocks: 'T-A45' });
		add('c-release-v1', 'kai', 'handoff', 25, 'Please review the store traits before I start the storage work.', { addressed_to: 'sam', linked_ref: 'ac-T-12' });
		const root = add('c-release-v1', 'avery', 'msg', 90, '@sam your part and status for Phase 1, please.');
		add('c-release-v1', 'sam', 'reply', 80, 'Storage is done. Waiting on the contract review.', { thread_root_id: root });
		add('c-release-v1', 'avery', 'msg', 70, 'Thanks. @kai can you take the review?', { thread_root_id: root });
		add('c-release-v1', 'kai', 'reply', 66, 'On it.', { thread_root_id: root, also_in_channel: true });
		add(general.id, 'leo', 'msg', 15, 'Can someone link the Atlas plan? (from Slack)', { bridge: 'slack' });
	}
	if (platform) add(platform, 'kai', 'announcement', 100, 'Blocked: waiting on credentials for the build host.', { reason: 'blocked' });

	// #release-v1, #design-crit
	add('c-release-v1', 'sam', 'announcement', 200, 'Delivered: the offices screen is in v1.', { reason: 'delivered' });
	add('c-design-crit', 'juno', 'msg', 300, 'Crit at 3pm, bring the empty states.');
	add('c-design-crit', 'pax', 'reply', 290, 'I will have them.');

	// DMs
	add('c-dm-sam', owner, 'msg', 140, 'How is create-org going?');
	add('c-dm-sam', 'sam', 'reply', 135, 'Done, and in review. I will only post when something needs you.');
	add('c-dm-avery', 'avery', 'msg', 400, 'Can we move the sync to Thursday?');
	add('c-group', owner, 'msg', 500, 'Sam, Noor: who takes the web part of ac-17?');
	add('c-group', 'noor', 'reply', 490, 'I do. Starting after lunch.');
	add('c-dm-sam-noor', 'sam', 'handoff', 600, 'Noor, please take the settings page.', { addressed_to: 'noor', linked_ref: 'ac-T-14', accepted_by: 'noor', accepted_at: at(595) });
	add('c-dm-sam-noor', 'noor', 'reply', 590, 'Accepted.');

	const presence: Record<string, Presence> = {};
	for (const m of ctx.members) presence[m.id] = m.kind === 'person' ? 'online' : 'offline';
	Object.assign(presence, { sam: 'working', noor: 'waiting', kai: 'working', mira: 'working', ines: 'asleep', pax: 'paused', juno: 'offline', leo: 'offline' } satisfies Record<string, Presence>);
	for (const id of Object.keys(presence)) if (!has(id)) delete presence[id];

	// The owner has read #general up to a few hours ago and answered nothing yet.
	const read_at = { [general.id]: at(60 * 3), 'c-dm-sam': at(130), 'c-group': at(480), 'c-dm-avery': at(300) };
	return { ...state, messages, presence, read_at, seq: 1000 };
}

/**
 * More of the sample, for the mock office only (the unit tests pin the base above): every card kind, a pinned message, a System
 * notice, Between agents, Browse channels, the HR lead and an offboarded agent, so each surface of chat-ux can be seen.
 * There are no bridges to other chat products (CHAT-17 withdrawn): the sample shows none.
 */
export function sampleChatFull(ctx: ChatContext): ChatState {
	const base = sampleChat(ctx);
	const at = (minutesAgo: number) => new Date(Date.parse(ctx.now) - minutesAgo * MIN).toISOString();
	const has = (id: string) => ctx.members.some((m) => m.id === id);
	const owner = ctx.owner;
	const conversations: Conversation[] = [];
	const dm = (id: string, members: string[]) => {
		if (members.every(has)) conversations.push({ id, kind: 'dm', members, created_at: at(60 * 48) });
	};
	if (['kai', 'sam', 'hana'].every(has)) conversations.push({ id: 'c-incident-drills', kind: 'channel', name: 'incident-drills', purpose: 'Game days for the build host and the office failover.', access: 'invite', members: ['kai', 'sam', 'hana'], created_at: at(60 * 40) });
	if (['kai', 'avery'].every(has)) conversations.push({ id: 'c-security-review', kind: 'channel', name: 'security-review', purpose: 'Findings under embargo.', access: 'restricted', members: ['kai', 'avery'], created_at: at(60 * 40) });
	dm('c-dm-noor', [owner, 'noor']);
	dm('c-dm-quinn', [owner, 'quinn']);
	dm('c-dm-kai-mira', ['kai', 'mira']);
	dm('c-dm-hr', [owner, 'hr']);
	dm('c-dm-marlowe', [owner, 'marlowe']);
	const state: ChatState = { ...base, conversations: [...base.conversations, ...conversations], messages: base.messages.map(({ bridge: _bridge, ...m }) => m) };
	const general = state.conversations.find((c) => c.auto?.type === 'general')!;
	const channelOf = (projectId: string) => state.conversations.find((c) => c.auto?.type === 'project' && c.auto.project_id === projectId)?.id;
	const atlas = channelOf('atlas');
	const messages: ChatMessage[] = [];
	let n = 0;
	const add = (conversation_id: string | undefined, from: string, kind: ChatMessage['kind'], minutesAgo: number, body: string, extra: Partial<ChatMessage> = {}): string => {
		const id = `m-x${++n}`;
		if (conversation_id && state.conversations.some((c) => c.id === conversation_id) && (from === OFFICE || has(from)) && (extra.addressed_to === undefined || has(extra.addressed_to))) messages.push({ id, conversation_id, from, kind, body, created_at: at(minutesAgo), ...extra });
		return id;
	};
	// ---- added for every card and state (appended, so the ids above stay put) ----
	const beacon = channelOf('beacon');
	// #general: a System notice and the pinned plan
	add(general.id, OFFICE, 'announcement', 60 * 4, 'Quinn Adeyemi joined the organization as a researcher on Atlas.', { reason: 'report' });
	add(general.id, 'avery', 'msg', 60 * 4 - 5, 'The Atlas plan for this quarter: [Atlas plan](https://example.com/docs/atlas-plan). Storage first, then the importer.', { pinned: true });
	if (atlas) {
		add(atlas, 'kai', 'announcement', 60 * 3, 'Report: the storage spike is done. Numbers and the decision are in T-A40.', { reason: 'report', pinned: true, reactions: { seen: [owner, 'mira'], looking: ['sam'] } });
		// An agent-to-agent ask its asker withdrew
		add(atlas, 'mira', 'request', 60 * 2, '@kai can I take the migration review off your plate?', { addressed_to: 'kai', options: [{ id: 'yes', label: 'Yes, take it' }, { id: 'no', label: 'No, I have it' }], withdrawn: { by: 'mira', at: at(60 * 2 - 20), reason: 'Sam took it.' } });
	}
	if (beacon) {
		// A form: four questions, so it shows as steps; one optional, defaults preselected
		add(beacon, 'noor', 'request', 18, 'A few choices before I build the Beacon first-run flow.', {
			addressed_to: owner,
			questions: [
				{ id: 'signup', question_type: 'choice', prompt: 'Which sign-up path ships first?', options: [{ id: 'email', label: 'Email link', description: 'Passwordless, one field. Works today.' }, { id: 'sso', label: 'Single sign-on', description: 'Google and Microsoft. Needs the admin setup first.' }], default_option: 'email' },
				{ id: 'tour', question_type: 'yes_no', prompt: 'Show the product tour on the first sign-in?', default_option: 'yes' },
				{ id: 'empty', question_type: 'choice', prompt: 'Which empty states are in the first cut?', allow_multiple: true, options: [{ id: 'projects', label: 'Projects' }, { id: 'teams', label: 'Teams' }, { id: 'chat', label: 'Chat' }] },
				{ id: 'avoid', question_type: 'free_text', prompt: 'Anything the welcome copy must avoid?', optional: true }
			],
			default_at: at(-45),
			wait_until: at(-45),
			blocks: 'T-B37'
		});
		// A default that was applied: nobody answered, the agent continued with it
		add(beacon, 'juno', 'request', 60 * 6, 'Use the darker card border on the Beacon dashboard?', { addressed_to: owner, options: [{ id: 'dark', label: 'Darker border', description: 'Reads better on the gray-800 page.' }, { id: 'keep', label: 'Keep the current one' }], default_option: 'dark', default_at: at(60 * 5), wait_until: at(60 * 5) });
		// A hand-off between agents, handed back with a reason
		const audit = add(beacon, 'pax', 'handoff', 60 * 7, '@juno take the icon audit for the settings pages?', { addressed_to: 'juno', linked_ref: 'T-B21', handed_back: { by: 'juno', at: at(60 * 7 - 30), reason: 'Needs the new 20 px grid first.' } });
		add(beacon, 'juno', 'reply', 60 * 7 - 30, 'Handed back: Needs the new 20 px grid first.', { thread_root_id: audit });
	}
	// #release-v1: an ask to someone else (the owner reads it, but only Avery answers)
	add('c-release-v1', 'sam', 'request', 30, '@avery can you confirm the release date with the pilot customers?', { addressed_to: 'avery', options: [{ id: 'fri', label: 'Friday holds' }, { id: 'mon', label: 'Move to Monday' }] });
	// DMs: a hand-off to the owner, a required ask whose agent is stopped, an answered ask, the HR lead, an offboarded agent
	add('c-dm-noor', 'noor', 'handoff', 22, 'Please take the final copy check on the Beacon sign-up page. The draft is in T-B37.', { addressed_to: owner, linked_ref: 'T-B37' });
	add('c-dm-quinn', 'quinn', 'request', 40, 'Which interview notes can I quote in the Atlas research summary?', { addressed_to: owner, required: true, wait_until: at(30), options: [{ id: 'anon', label: 'Only the anonymised ones' }, { id: 'all', label: 'All, with names', description: 'Every participant signed the consent form.' }], blocks: 'T-A47' });
	add('c-dm-sam', 'sam', 'request', 60 * 3, 'Ship the create-org flow behind a flag, or wait for the review?', { addressed_to: owner, options: [{ id: 'flag', label: 'Behind a flag' }, { id: 'wait', label: 'Wait for the review' }], answer: { by: owner, at: at(60 * 3 - 8), option_id: 'flag' } });
	add('c-dm-sam', owner, 'reply', 60 * 3 - 8, 'Behind a flag');
	add('c-dm-kai-mira', 'kai', 'msg', 70, 'Mira, the storage traits are in review. Can you pair on the importer after?');
	add('c-dm-kai-mira', 'mira', 'reply', 65, 'Yes, after lunch.');
	add('c-dm-hr', owner, 'msg', 60 * 26, 'Can you draft a hire for a QA agent on Atlas?');
	add('c-dm-hr', 'hr', 'reply', 60 * 26 - 3, 'Drafted: a QA engineer in Platform, reporting to Kai. It is in Hire, waiting for your review.');
	add('c-dm-marlowe', 'marlowe', 'msg', 60 * 50, 'The importer spike is parked in T-A12. Notes are in the task.');
	add('c-dm-marlowe', 'marlowe', 'request', 60 * 49, 'Keep the importer spike branch, or delete it?', { addressed_to: owner, options: [{ id: 'keep', label: 'Keep it' }, { id: 'delete', label: 'Delete it' }], withdrawn: { by: OFFICE, at: at(60 * 30), reason: 'Marlowe left the organization' } });

	const presence: Record<string, Presence> = { ...state.presence };
	if (has('quinn')) presence.quinn = 'waiting';
	if (has('hr')) presence.hr = 'asleep';
	const read_at = { ...state.read_at, 'c-dm-hr': at(60 * 20), 'c-dm-marlowe': at(60 * 20), 'c-dm-sam': at(60) };
	return { ...state, messages: [...state.messages, ...messages], presence, read_at };
}
