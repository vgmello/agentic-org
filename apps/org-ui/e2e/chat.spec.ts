import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page, query = '') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/chat${query}`);
	await expect(page.getByRole('navigation', { name: 'Chat' })).toBeVisible();
}
const side = (page: Page) => page.getByRole('navigation', { name: 'Chat' });
const go = (page: Page, name: string | RegExp) => side(page).getByTestId('conversation-link').filter({ hasText: name }).first().click();
const title = (page: Page) => page.getByTestId('conversation-title');
const messages = (page: Page) => page.getByTestId('message');
const composer = (page: Page) => page.getByTestId('composer').first();
const send = async (page: Page, text: string) => {
	await composer(page).fill(text);
	await composer(page).press('Control+Enter');
};

test('CHAT-1: Chat is in the nav, with Needs you, Mentions, Threads, channels and DMs, and a conversation with threads', async ({ page }) => {
	await open(page);
	await expect(page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Chat' })).toHaveAttribute('aria-current', 'page');
	await expect(side(page).getByTestId('needs-you-count')).toHaveText('2');
	await expect(side(page).getByTestId('mentions-link')).toBeVisible();
	await expect(side(page).getByTestId('threads-link')).toBeVisible();
	await expect(side(page).getByTestId('channels')).toContainText('general');
	await expect(side(page).getByTestId('channels')).toContainText('atlas');
	await expect(side(page).getByTestId('dms')).toContainText('Sam Okafor');
	await expect(page.getByTestId('sample-data-note')).toHaveText('Sample data');
	await go(page, 'release-v1');
	await expect(title(page)).toHaveText('#release-v1');
	await shot(page, 'chat-channel');
	await page.getByTestId('replies-link').first().click();
	const thread = page.getByTestId('thread-panel');
	await expect(thread).toContainText('3 replies');
	await expect(thread).toContainText('Storage is done');
	await shot(page, 'chat-thread');
	await thread.getByRole('button', { name: 'Close thread' }).click();
	await expect(thread).toBeHidden();
});

test('CHAT-2: #general, project and team channels exist, and people add a channel and invite members', async ({ page }) => {
	await open(page);
	for (const name of ['general', 'atlas', 'beacon', 'engineering', 'web', 'platform']) await expect(side(page).getByTestId('channels')).toContainText(name);
	await side(page).getByRole('button', { name: 'Add channel' }).click();
	const dialog = page.getByRole('dialog', { name: 'Add a channel' });
	await dialog.getByLabel('Name').fill('Release V2');
	await expect(dialog).toContainText('It will be #release-v2.');
	await dialog.getByLabel('Sam Okafor').check();
	await shot(page, 'chat-add-channel', { context: dialog });
	await dialog.getByRole('button', { name: 'Add channel' }).click();
	await expect(dialog).toBeHidden();
	await expect(title(page)).toHaveText('#release-v2');
	await page.getByRole('button', { name: 'Members' }).click();
	await expect(page.getByTestId('member-row')).toHaveCount(2);
	await page.getByRole('button', { name: 'Add people…' }).click();
	await page.getByRole('dialog', { name: 'Add people' }).getByLabel('Kai Lindqvist').check();
	await page.getByRole('button', { name: 'Save members' }).click();
	await expect(page.getByTestId('member-row')).toHaveCount(3);
	await shot(page, 'chat-channel-members');
	// An automatic channel follows the organization and cannot be edited by hand.
	await go(page, 'atlas');
	await page.getByRole('button', { name: 'Members' }).click();
	await expect(page.getByTestId('members-panel')).toContainText('follows the Atlas project');
	await expect(page.getByRole('button', { name: 'Add people…' })).toHaveCount(0);
});

test('CHAT-3: DMs show each member\'s status, and a group of up to 8 can be started', async ({ page }) => {
	await open(page);
	const dms = side(page).getByTestId('dms');
	await expect(dms.locator('[data-conversation=c-dm-sam]')).toContainText('Working');
	await expect(dms.locator('[data-conversation=c-dm-ines]')).toContainText('Asleep');
	await expect(dms.locator('[data-conversation=c-dm-pax]')).toContainText('Paused');
	// A person's status is a word too, never only a dot (CHAT-18).
	await expect(dms.locator('[data-conversation=c-dm-avery]').getByTestId('dm-status')).toHaveText('Online');
	await side(page).getByRole('button', { name: 'New message' }).click();
	const dialog = page.getByRole('dialog', { name: 'New message' });
	for (const n of ['Avery Stone', 'Hana Weiss', 'Leo Brandt', 'Kai Lindqvist', 'Mira Castillo', 'Juno Park', 'Pax Moreau']) await dialog.getByLabel(n).check();
	await expect(dialog.getByLabel('Noor Haddad')).toBeDisabled();
	await shot(page, 'chat-new-group-limit', { context: dialog });
	await dialog.getByRole('button', { name: 'Start group' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByRole('button', { name: 'Members' })).toBeVisible();
	await page.getByRole('button', { name: 'Members' }).click();
	await expect(page.getByTestId('member-row')).toHaveCount(8);
});

test('CHAT-5: an ask is a card with its options; answering it answers the ask and clears Needs you', async ({ page }) => {
	await open(page);
	await expect(title(page)).toHaveText('Needs you');
	const card = page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
	await expect(card).toContainText('Kai Lindqvist asks you');
	await expect(card).toContainText('blocks W7');
	await expect(card.getByTestId('waiting')).toHaveText(/Waiting \d+ min/);
	await shot(page, 'chat-needs-you');
	await card.getByRole('button', { name: 'Yes, block' }).click();
	await expect(page.getByText('Answer sent to Kai Lindqvist.')).toBeVisible();
	// It leaves Needs you at once, and the card in its channel says who answered.
	await expect(card).toHaveCount(0);
	await expect(side(page).getByTestId('needs-you-count')).toHaveText('1');
	await go(page, 'atlas');
	const answered = page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
	await expect(answered).toHaveAttribute('data-answered', 'true');
	await expect(answered.getByTestId('answered')).toHaveText('Answered by Alex Kim: Yes, block');
	await expect(answered.getByRole('button', { name: 'Yes, block' })).toHaveCount(0);
	await shot(page, 'chat-ask-answered');
});

test('CHAT-5: Reply… answers with text, and an approval is rejected with a reason', async ({ page }) => {
	await open(page);
	const ask = page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
	await ask.getByRole('button', { name: 'Reply…' }).click();
	await ask.getByLabel('Your reply').fill('Block, but warn first.');
	await ask.getByRole('button', { name: 'Send reply' }).click();
	await expect(ask).toHaveCount(0);
	await go(page, 'atlas');
	await expect(page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' }).getByTestId('answered')).toContainText('Block, but warn first.');
	await side(page).getByTestId('needs-you-link').click();
	const approval = page.getByTestId('ask-card').filter({ hasText: 'Publish the 0.2 release notes' });
	await expect(approval).toContainText('Approval');
	await approval.getByRole('button', { name: 'Reject…' }).click();
	await expect(approval.getByRole('button', { name: 'Reject', exact: true })).toBeDisabled();
	await approval.getByLabel('Why do you reject it?').fill('Not before the audit.');
	await approval.getByRole('button', { name: 'Reject', exact: true }).click();
	await expect(page.getByTestId('nothing-needs-you')).toBeVisible();
	await shot(page, 'chat-needs-you-empty');
	await go(page, 'atlas');
	await expect(page.getByTestId('ask-card').filter({ hasText: 'Publish the 0.2' }).getByTestId('answered')).toContainText('Answered by Alex Kim: Reject · Not before the audit.');
});

test('CHAT-18: only a hand-off\'s recipient sees Accept, and an accepted one reads who and when with no buttons', async ({ page }) => {
	await open(page);
	// Kai handed this to Sam: the owner reading it sees Reply… only.
	await go(page, 'release-v1');
	const handoff = page.getByTestId('handoff-card').filter({ hasText: 'review the store traits' });
	await expect(handoff).toBeVisible();
	await expect(handoff.getByRole('button', { name: 'Accept' })).toHaveCount(0);
	await expect(handoff.getByRole('button', { name: 'Reply…' })).toBeVisible();
	// Sam handed one to Noor, who accepted it.
	await side(page).getByTestId('as-owner').locator('summary').click();
	await side(page).getByTestId('as-owner').getByTestId('conversation-link').filter({ hasText: 'Sam Okafor, Noor Haddad' }).click();
	const done = page.getByTestId('handoff-card').filter({ hasText: 'take the settings page' });
	await expect(done.getByTestId('answered')).toHaveText(/^Accepted by Noor Haddad · \d\d:\d\d$/);
	await expect(done.getByRole('button')).toHaveCount(0);
});

test('CHAT-6: an ask with a default shows its deadline', async ({ page }) => {
	await open(page);
	await expect(page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' }).getByTestId('deadline')).toContainText("the default applies: No, allow (default)");
});

test('CHAT-7: messaging an asleep agent says it will wake it, and a typing indicator shows while it works', async ({ page }) => {
	await open(page);
	await go(page, 'Inês Duarte');
	await expect(page.getByTestId('composer-note')).toHaveText('Inês Duarte is asleep. Your message will wake it.');
	await send(page, 'Can you pull the headcount?');
	await expect(page.getByTestId('typing')).toHaveText('Inês Duarte is typing…');
	await expect(messages(page).filter({ hasText: 'Sample reply' })).toHaveCount(1);
	await expect(page.getByTestId('typing')).not.toContainText('typing');
	await expect(side(page).locator('[data-conversation=c-dm-ines]')).toContainText('Working');
	await expect(page.getByTestId('composer-note')).toHaveCount(0);
});

test('CHAT-8: a thread reply can also be sent to the channel', async ({ page }) => {
	await open(page);
	await go(page, 'release-v1');
	await page.getByTestId('replies-link').first().click();
	const thread = page.getByTestId('thread-panel');
	await thread.getByTestId('composer').fill('FYI, shipping Friday');
	await thread.getByLabel('Also send to #release-v1').check();
	await thread.getByTestId('composer').press('Control+Enter');
	await expect(thread).toContainText('FYI, shipping Friday');
	await expect(page.getByTestId('message-list').getByText('FYI, shipping Friday')).toHaveCount(1);
	await thread.getByTestId('composer').fill('Only in the thread');
	await thread.getByTestId('composer').press('Control+Enter');
	await expect(thread).toContainText('Only in the thread');
	await expect(page.getByTestId('message-list').getByText('Only in the thread')).toHaveCount(0);
});

test('CHAT-9: /bug, /request, /task, /status, /wake and /remind open their forms, and Turn into… does the same', async ({ page }) => {
	await open(page);
	await go(page, 'general');
	await send(page, '/bug the export button does nothing');
	let dialog = page.getByRole('dialog', { name: 'Report a bug' });
	await expect(dialog.getByLabel('Title')).toHaveValue('the export button does nothing');
	await shot(page, 'chat-command-form', { context: dialog });
	await dialog.getByLabel('What happened').fill('Click it, nothing happens.');
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(page.getByText('drafted for Atlas. Sample data: nothing was saved.')).toBeVisible();
	await send(page, '/request dark mode');
	await expect(page.getByRole('dialog', { name: 'Send a feature request' }).getByLabel('What do you need')).toHaveValue('dark mode');
	await page.keyboard.press('Escape');
	await send(page, '/task @sam write the docs');
	dialog = page.getByRole('dialog', { name: 'Assign a task to Sam Okafor' });
	await expect(dialog.getByLabel('Assignee')).toHaveValue('sam');
	await page.keyboard.press('Escape');
	await send(page, '/status beacon');
	await expect(page.getByRole('dialog', { name: 'Post the project status' }).getByLabel('Project')).toHaveValue('beacon');
	await page.keyboard.press('Escape');
	await send(page, '/wake @ines');
	await expect(page.getByRole('dialog', { name: 'Run Inês Duarte now' })).toBeVisible();
	await page.keyboard.press('Escape');
	await send(page, '/remind in 2h check the build');
	await expect(page.getByRole('dialog', { name: 'Remind me' }).getByLabel('About')).toHaveValue('check the build');
	await page.keyboard.press('Escape');
	// A command that is missing something says so and keeps the text.
	await send(page, '/task');
	await expect(page.getByText('Say which agent: /task @agent [title]')).toBeVisible();
	await expect(composer(page)).toHaveValue('/task');
	await composer(page).fill('');
	// Turn into… on a message.
	const row = messages(page).filter({ hasText: 'Ready for review: create-org' });
	await row.hover();
	await row.getByTestId('message-menu').click();
	await page.getByRole('menuitem', { name: 'Turn into a bug…' }).click();
	await expect(page.getByRole('dialog', { name: 'Report a bug' }).getByLabel('Title')).toHaveValue('Ready for review: create-org branch at 9c1e0b2.');
});

test('CHAT-10: extra announcements are folded into a thread under the last one', async ({ page }) => {
	await open(page);
	await go(page, 'general');
	const root = messages(page).filter({ hasText: 'Ready for review: create-org' });
	await expect(root.getByTestId('replies-link')).toHaveText('↳ 3 replies · Sam Okafor posted 3 more updates');
	await root.getByTestId('replies-link').click();
	await expect(page.getByTestId('thread-panel')).toContainText('Merged the review fixes.');
	// The folded updates are not in the channel itself.
	await expect(page.getByTestId('message-list').getByText('Merged the review fixes.')).toHaveCount(0);
});

test('CHAT-11: the owner mutes an agent in a channel or sets a digest (that muted messages still reach Activity is in the model tests)', async ({ page }) => {
	await open(page);
	await go(page, 'general');
	const row = messages(page).filter({ hasText: 'Ready for review: create-org' });
	await row.hover();
	await row.getByTestId('message-menu').click();
	await page.getByRole('menuitem', { name: 'Mute Sam Okafor here' }).click();
	await expect(page.getByText('Sam Okafor is muted here. Their messages still reach Activity.')).toBeVisible();
	await expect(messages(page).filter({ hasText: 'Ready for review: create-org' })).toHaveCount(0);
	const muted = page.getByTestId('muted-row');
	await expect(muted).toContainText('1 muted message');
	await muted.getByRole('button').click();
	await expect(muted).toContainText('Ready for review: create-org');
	// Back to everything, then a digest from the Members panel.
	await page.getByRole('button', { name: 'Members' }).click();
	await page.getByTestId('member-row').filter({ hasText: 'Sam Okafor' }).getByTestId('mute-select').selectOption('digest');
	await expect(page.getByTestId('digest-row')).toContainText('Daily digest · Sam Okafor · 1 update');
});

test('CHAT-12: a conversation the owner is not in is read as the owner, and the owner can reply', async ({ page }) => {
	await open(page);
	await side(page).getByTestId('as-owner').locator('summary').click();
	await side(page).getByTestId('as-owner').getByTestId('conversation-link').filter({ hasText: 'Sam Okafor, Noor Haddad' }).click();
	await expect(page.getByTestId('reading-as-owner')).toHaveText("You're reading as the owner. You can reply here.");
	await send(page, 'Carry on.');
	await expect(messages(page).filter({ hasText: 'Carry on.' })).toHaveCount(1);
	await shot(page, 'chat-as-owner');
});

test('CHAT-13: search filters by text, member, channel, kind and date', async ({ page }) => {
	await open(page);
	const search = page.getByTestId('chat-search');
	const count = page.getByTestId('search-count');
	await search.fill('release notes');
	await expect(count).toContainText('1 result');
	await search.fill('from:@kai');
	await expect(count).toContainText('4 results');
	await expect(page.getByTestId('search-help')).toContainText('from:@name');
	await search.fill('has:ask');
	await expect(count).toContainText('2 results');
	await search.fill('in:#release-v1 from:@kai');
	await expect(count).toContainText('2 results');
	await search.fill('kind:announcement');
	await expect(count).toContainText('results');
	await search.fill('before:2020-01-01');
	await expect(page.getByTestId('no-results')).toBeVisible();
	await shot(page, 'chat-search-empty');
	await search.fill('from:@kai has:ask');
	await shot(page, 'chat-search');
	await page.getByTestId('message').first().getByRole('button', { name: 'Open in conversation' }).click();
	await expect(search).toHaveValue('');
	await expect(title(page)).toHaveText('#atlas');
});

test('CHAT-14: Markdown renders safely, item numbers are chips, long messages collapse, and @ completes a name to its handle', async ({ page }) => {
	await open(page);
	await go(page, 'general');
	const ready = messages(page).filter({ hasText: 'Ready for review: create-org' });
	await expect(ready.locator('strong')).toHaveText('create-org');
	await expect(ready.locator('code')).toHaveText('9c1e0b2');
	await expect(ready.getByTestId('item-chip')).toHaveText('ac-T-17');
	await expect(ready.getByRole('link', { name: 'the branch' })).toHaveAttribute('href', 'https://example.com/branch/create-org');
	await expect(ready.getByRole('link', { name: 'the branch' })).toHaveAttribute('rel', 'noopener noreferrer');
	// More than 15 lines collapses.
	const plan = messages(page).filter({ hasText: 'Plan for the week' });
	await expect(plan).toContainText('Show more (6 more lines)');
	await expect(plan).not.toContainText('Questions in the thread');
	await plan.getByRole('button', { name: /Show more/ }).click();
	await expect(plan).toContainText('Questions in the thread');
	await plan.getByRole('button', { name: 'Show less' }).click();
	await expect(plan).not.toContainText('Questions in the thread');
	// HTML typed in a message stays text.
	await send(page, '<img src=x onerror=alert(1)> and [x](javascript:alert(1))');
	const own = messages(page).filter({ hasText: 'onerror' });
	await expect(own).toContainText('<img src=x onerror=alert(1)>');
	await expect(own.locator('img')).toHaveCount(0);
	await expect(own.locator('a')).toHaveCount(0);
	await shot(page, 'chat-safe-markdown');
	// Mentions complete a typed name to the handle.
	await composer(page).fill('hello @hana');
	await expect(page.getByTestId('composer-popup')).toContainText('Hana Weiss');
	await composer(page).press('Enter');
	await expect(composer(page)).toHaveValue('hello @hana-weiss ');
});

test('CHAT-15: the link an Activity row uses (/chat?c=…&m=…) opens the message in Chat', async ({ page }) => {
	await open(page);
	await go(page, 'atlas');
	const id = await messages(page).filter({ hasText: 'Publish the 0.2' }).getAttribute('data-id');
	await visit(page, `/chat?c=c-project-atlas&m=${id}`);
	await expect(title(page)).toHaveText('#atlas');
	await expect(messages(page).filter({ hasText: 'Publish the 0.2' })).toBeInViewport();
	await visit(page, '/chat?c=c-dm-sam');
	await expect(title(page)).toHaveText('Sam Okafor');
});

test('CHAT-17: a bridged message shows its platform, and asks to the owner and approvals are read-only on a bridge', async ({ page }) => {
	await open(page);
	await go(page, 'general');
	const bridged = messages(page).filter({ hasText: 'Can someone link the Atlas plan' });
	await expect(bridged).toContainText('Leo Brandt');
	await expect(bridged.getByTestId('bridge-badge')).toHaveText('Slack');
	await visit(page, '/chat?surface=bridge');
	await expect(page.getByTestId('bridge-preview')).toBeVisible();
	const card = page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
	await expect(card.getByTestId('app-only')).toContainText('Open in Agentic Org');
	await expect(card.getByRole('button', { name: 'Yes, block' })).toHaveCount(0);
	await expect(page.getByTestId('ask-card').filter({ hasText: 'Publish the 0.2' }).getByTestId('app-only')).toBeVisible();
	// Inside a thread too, and the composer is off.
	await go(page, 'release-v1');
	await page.getByTestId('replies-link').first().click();
	await expect(page.getByTestId('thread-panel').getByTestId('composer')).toHaveCount(0);
	await shot(page, 'chat-bridge-preview');
});

test('states: the office is not running, a message that fails shows Not delivered with Retry, and an empty organization has a welcome card', async ({ page }) => {
	await open(page, '?chat=offline');
	await go(page, 'general');
	await expect(page.getByTestId('office-down')).toBeVisible();
	await expect(page.getByTestId('composer-note')).toHaveText("The office isn't running. Restart it from Offices.");
	await expect(composer(page)).toHaveCount(0);
	await shot(page, 'chat-office-offline');

	await visit(page, '/chat?chat=fail');
	await go(page, 'general');
	await send(page, 'Will this arrive?');
	await expect(page.getByTestId('not-delivered')).toContainText("Not delivered: The office didn't answer.");
	await shot(page, 'chat-delivery-failed');
	await page.getByRole('button', { name: 'Retry' }).click();
	await expect(page.getByTestId('not-delivered')).toHaveCount(0);
	await expect(messages(page).filter({ hasText: 'Will this arrive?' })).toHaveCount(1);
	await shot(page, 'chat-delivery-retried');

	await visit(page, '/chat?org=empty');
	await expect(title(page)).toHaveText('#general');
	await expect(page.getByTestId('welcome-card')).toContainText('Agents and people talk here.');
	await shot(page, 'chat-empty');
});

// Hold the existing organization read used by Chat's snapshot, without a production test hook.
type HeldChatRead = { started: Promise<void>; release: () => void };

for (const fail of [false, true]) {
	test(`CHAT-1: the initial snapshot shows loading, then ${fail ? 'its error' : 'the conversation list'}`, async ({ page }) => {
		await open(page);
		await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Agents', exact: true }).click();
		await expect(page).toHaveURL(/\/agents$/);
		await page.evaluate(async (fail) => {
			const modulePath = '/src/lib/org/api.ts';
			const { orgApi } = await import(modulePath);
			const view = orgApi.view.bind(orgApi);
			let started!: () => void;
			let release!: () => void;
			const began = new Promise<void>((resolve) => (started = resolve));
			const gate = new Promise<void>((resolve) => (release = resolve));
			orgApi.view = async (organizationId: string) => {
				orgApi.view = view;
				started();
				await gate;
				if (fail) throw { source: 'api', code: 'unavailable', message: 'The office went away.' };
				return view(organizationId);
			};
			(window as unknown as { __heldChatRead: HeldChatRead }).__heldChatRead = { started: began, release };
		}, fail);
		// Client navigation retains the same dev-module instance as Chat's snapshot adapter.
		await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Chat', exact: true }).click();
		await page.evaluate(() => (window as unknown as { __heldChatRead: HeldChatRead }).__heldChatRead.started);
		await expect(page.getByRole('status')).toHaveText('Loading chat…');
		await shot(page, 'chat-snapshot-loading', { settle: false });
		await page.evaluate(() => (window as unknown as { __heldChatRead: HeldChatRead }).__heldChatRead.release());
		if (fail) {
			await expect(page.getByRole('alert')).toHaveText('The office went away.');
			await expect(side(page)).toHaveCount(0);
		} else {
			await expect(side(page)).toBeVisible();
			await expect(title(page)).toHaveText('Needs you');
		}
		await shot(page, fail ? 'chat-snapshot-error' : 'chat-snapshot-loaded');
	});
}
