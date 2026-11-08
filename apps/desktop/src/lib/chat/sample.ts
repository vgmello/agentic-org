// A sample conversation history for the mock office: invented, none from a real workspace. It is built on the sample
// organization (src/lib/org/sample.ts), and any message whose people aren't in the organization is left out, so the
// organization that has only its owner gets #general and nothing else.
import { APPROVAL_OPTIONS, emptyChat, syncAuto, type ChatContext } from './engine';
import type { ChatMessage, ChatState, Conversation, Presence } from './types';

const MIN = 60_000;

export function sampleChat(ctx: ChatContext): ChatState {
	const at = (minutesAgo: number) => new Date(Date.parse(ctx.now) - minutesAgo * MIN).toISOString();
	const has = (id: string) => ctx.members.some((m) => m.id === id);
	let state = syncAuto(emptyChat(), { ...ctx, newId: undefined });
	const owner = ctx.owner;
	const conversations: Conversation[] = [];
	const dm = (id: string, members: string[], kind: Conversation['kind'] = 'dm') => {
		if (members.every(has)) conversations.push({ id, kind, members, created_at: at(60 * 48) });
	};
	if (['sam', 'noor', 'avery'].every(has)) conversations.push({ id: 'c-release-v1', kind: 'channel', name: 'release-v1', members: [owner, 'sam', 'kai', 'avery'], created_at: at(60 * 30) });
	if (['juno', 'pax', 'noor'].every(has)) conversations.push({ id: 'c-design-crit', kind: 'channel', name: 'design-crit', members: ['juno', 'pax', 'noor'], created_at: at(60 * 30) });
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
		if (conversation_id && has(from) && (extra.addressed_to === undefined || has(extra.addressed_to))) messages.push({ id, conversation_id, from, kind, body, created_at: at(minutesAgo), ...extra });
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
				{ id: 'no', label: "No, allow (Eden's AREA-7)" }
			],
			default_option: 'no',
			default_at: at(-120),
			blocks: 'W7'
		});
		add(atlas, 'mira', 'request', 9, 'Publish the 0.2 release notes? They list REQ-14 and ADR-0004.', { addressed_to: owner, approval: true, options: APPROVAL_OPTIONS, blocks: 'release' });
		add('c-release-v1', 'kai', 'handoff', 25, 'Please review the store traits before I start the storage work.', { addressed_to: 'sam', linked_ref: 'T-12' });
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
	add('c-dm-sam-noor', 'sam', 'handoff', 600, 'Noor, please take the settings page.', { addressed_to: 'noor', linked_ref: 'T-14' });
	add('c-dm-sam-noor', 'noor', 'reply', 590, 'Accepted.');

	const presence: Record<string, Presence> = {};
	for (const m of ctx.members) presence[m.id] = m.kind === 'person' ? 'online' : 'offline';
	Object.assign(presence, { sam: 'working', noor: 'waiting', kai: 'working', mira: 'working', ines: 'asleep', pax: 'paused', juno: 'offline', leo: 'offline' } satisfies Record<string, Presence>);
	for (const id of Object.keys(presence)) if (!has(id)) delete presence[id];

	// The owner has read #general up to a few hours ago and answered nothing yet.
	const read_at = { [general.id]: at(60 * 3), 'c-dm-sam': at(130), 'c-group': at(480), 'c-dm-avery': at(300) };
	return { ...state, messages, presence, read_at, seq: 1000 };
}
