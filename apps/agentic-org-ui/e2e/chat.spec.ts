import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

async function open(page: Page, query = '') {
	await enterMockedOrganization(page);
	await expect(page).toHaveURL(/\/teams\/members$/);
	await visit(page, `/chat${query}`);
	await expect(page.getByRole('navigation', { name: 'Chat' })).toBeVisible();
}
const side = (page: Page) => page.getByRole('navigation', { name: 'Chat' });
const go = (page: Page, name: string | RegExp) =>
	side(page).getByTestId('conversation-link').filter({ hasText: name }).first().click();
const title = (page: Page) => page.getByTestId('conversation-title');
const messages = (page: Page) => page.getByTestId('message');
const composer = (page: Page) => page.getByTestId('composer').first();
const send = async (page: Page, text: string) => {
	await composer(page).fill(text);
	await composer(page).press('Control+Enter');
};

test(
	'CHAT-1: Chat is in the nav, with Inbox link, Mentions, Threads, channels and DMs, and a conversation with threads',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page);
		await expect(
			page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Chat' })
		).toHaveAttribute('aria-current', 'page');
		await expect(title(page)).toHaveText('#general');
		await expect(side(page).getByTestId('inbox-link')).toHaveAttribute('href', '/inbox');
		await expect(side(page).getByTestId('inbox-count')).toHaveText('3');
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
	}
);

test('CHAT-2: #general, project and team channels exist, and people add a channel and invite members', async ({
	page
}) => {
	await open(page);
	for (const name of ['general', 'atlas', 'beacon', 'engineering', 'web', 'platform'])
		await expect(side(page).getByTestId('channels')).toContainText(name);
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

test("CHAT-3: DMs show each agent's status and no speculative presence for people, and a group of up to 8 can be started", async ({
	page
}) => {
	await open(page);
	const dms = side(page).getByTestId('dms');
	await expect(dms.locator('[data-conversation=c-dm-sam]')).toContainText('Working');
	await expect(dms.locator('[data-conversation=c-dm-ines]')).toContainText('Available');
	await expect(dms.locator('[data-conversation=c-dm-pax]')).toContainText('Paused');
	await expect(dms.locator('[data-conversation=c-dm-avery]').getByTestId('dm-status')).toHaveCount(
		0
	);
	await side(page).getByRole('button', { name: 'New message' }).click();
	const dialog = page.getByRole('dialog', { name: 'New message' });
	for (const n of [
		'Avery Stone',
		'Hana Weiss',
		'Leo Brandt',
		'Kai Lindqvist',
		'Mira Castillo',
		'Juno Park',
		'Pax Moreau'
	])
		await dialog.getByLabel(n).check();
	await expect(dialog.getByLabel('Noor Haddad')).toBeDisabled();
	await shot(page, 'chat-new-group-limit', { context: dialog });
	await dialog.getByRole('button', { name: 'Start group' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByRole('button', { name: 'Members' })).toBeVisible();
	await page.getByRole('button', { name: 'Members' }).click();
	await expect(page.getByTestId('member-row')).toHaveCount(8);
});

test(
	'CHAT-5: an ask is a card with its options; answering it records who answered and removes buttons',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page);
		await go(page, 'atlas');
		const card = page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
		await expect(card).toContainText('Kai Lindqvist asks you');
		await expect(card).toContainText('blocks T-A42');
		await expect(card.getByTestId('waiting')).toHaveText(/Waiting \d+ min/);
		await card.getByRole('button', { name: 'Yes, block' }).click();
		await expect(page.getByText('Answer sent to Kai Lindqvist.')).toBeVisible();
		await expect(card).toHaveAttribute('data-answered', 'true');
		await expect(card.getByTestId('answered')).toHaveText(
			/^Answered by you · \d\d:\d\d: Yes, block$/
		);
		await expect(card.getByRole('button', { name: 'Yes, block' })).toHaveCount(0);
		await shot(page, 'chat-ask-answered');
	}
);

test('CHAT-5: Reply… answers with text, and an approval is rejected with a reason', async ({
	page
}) => {
	await open(page);
	await go(page, 'atlas');
	const ask = page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
	await ask.getByRole('button', { name: 'Reply…' }).click();
	await ask.getByLabel('Your reply').fill('Block, but warn first.');
	await ask.getByRole('button', { name: 'Send reply' }).click();
	await expect(ask.getByTestId('answered')).toContainText('Block, but warn first.');

	const approval = page
		.getByTestId('ask-card')
		.filter({ hasText: 'Publish the 0.2 release notes' });
	await expect(approval).toContainText('Approval');
	await approval.getByRole('button', { name: 'Reject…' }).click();
	await expect(approval.getByRole('button', { name: 'Reject', exact: true })).toBeDisabled();
	await approval.getByLabel('Why do you reject it?').fill('Not before the audit.');
	await approval.getByRole('button', { name: 'Reject', exact: true }).click();
	await expect(approval.getByTestId('answered')).toContainText('Reject: Not before the audit.');
});

test("CHAT-18: only a hand-off's recipient sees Accept and Hand back…, and an accepted one reads who and when", async ({
	page
}) => {
	await open(page);
	// Kai handed this to Sam: the owner reading it sees Reply… only.
	await go(page, 'release-v1');
	const handoff = page.getByTestId('handoff-card').filter({ hasText: 'review the store traits' });
	await expect(handoff).toBeVisible();
	await expect(handoff.getByRole('button', { name: 'Accept' })).toHaveCount(0);
	await expect(handoff.getByRole('button', { name: 'Hand back…' })).toHaveCount(0);
	await expect(handoff.getByRole('button', { name: 'Reply…' })).toBeVisible();
	// Sam handed one to Noor, who accepted it.
	await side(page)
		.getByTestId('as-owner')
		.getByTestId('conversation-link')
		.filter({ hasText: 'Sam Okafor, Noor Haddad' })
		.click();
	const done = page.getByTestId('handoff-card').filter({ hasText: 'take the settings page' });
	await expect(done.getByTestId('answered')).toHaveText(/^Accepted by Noor Haddad · \d\d:\d\d$/);
	await expect(done.getByRole('button')).toHaveCount(0);
});

test('CHAT-6: an ask with a default shows its deadline', async ({ page }) => {
	await open(page);
	await go(page, 'atlas');
	await expect(
		page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' }).getByTestId('deadline')
	).toContainText('the default applies: No, allow (default)');
});

test(
	'CHAT-7: messaging an asleep agent wakes it with a typing indicator while it works',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page);
		await go(page, 'Inês Duarte');
		await expect(page.getByTestId('composer-note')).toHaveCount(0);
		await send(page, 'Can you pull the headcount?');
		await expect(page.getByTestId('typing')).toHaveText('Inês Duarte is typing…');
		await expect(messages(page).filter({ hasText: 'Sample reply' })).toHaveCount(1);
		await expect(page.getByTestId('typing')).not.toContainText('typing');
		await expect(side(page).locator('[data-conversation=c-dm-ines]')).toContainText('Working');
	}
);

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

test('CHAT-9: /bug, /request, /task, /status, /wake and /remind open their forms, and Turn into… does the same', async ({
	page
}) => {
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
	await expect(
		page.getByRole('dialog', { name: 'Send a feature request' }).getByLabel('What do you need')
	).toHaveValue('dark mode');
	await page.keyboard.press('Escape');
	await send(page, '/task @sam write the docs');
	dialog = page.getByRole('dialog', { name: 'Assign a task to Sam Okafor' });
	await expect(dialog.getByLabel('Assignee')).toHaveValue('sam');
	await page.keyboard.press('Escape');
	await send(page, '/status beacon');
	await expect(
		page.getByRole('dialog', { name: 'Post the project status' }).getByLabel('Project')
	).toHaveValue('beacon');
	await page.keyboard.press('Escape');
	await send(page, '/wake @ines');
	await expect(page.getByRole('dialog', { name: 'Run Inês Duarte now' })).toBeVisible();
	await page.keyboard.press('Escape');
	await send(page, '/remind in 2h check the build');
	await expect(page.getByRole('dialog', { name: 'Remind me' }).getByLabel('About')).toHaveValue(
		'check the build'
	);
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
	await expect(page.getByRole('dialog', { name: 'Report a bug' }).getByLabel('Title')).toHaveValue(
		'Ready for review: create-org branch at 9c1e0b2.'
	);
});

test('CHAT-10: extra announcements are folded into a thread under the last one', async ({
	page
}) => {
	await open(page);
	await go(page, 'general');
	const root = messages(page).filter({ hasText: 'Ready for review: create-org' });
	await expect(root.getByTestId('replies-link')).toHaveText(
		'3 replies · Sam Okafor posted 3 more updates'
	);
	await root.getByTestId('replies-link').click();
	await expect(page.getByTestId('thread-panel')).toContainText('Merged the review fixes.');
	// The folded updates are not in the channel itself.
	await expect(page.getByTestId('message-list').getByText('Merged the review fixes.')).toHaveCount(
		0
	);
});

test('CHAT-11: the owner mutes an agent in a channel or sets a digest', async ({ page }) => {
	await open(page);
	await go(page, 'general');
	const row = messages(page).filter({ hasText: 'Ready for review: create-org' });
	await row.hover();
	await row.getByTestId('message-menu').click();
	await page.getByRole('menuitem', { name: 'Mute Sam Okafor here' }).click();
	await expect(
		page.getByText('Sam Okafor is muted here. Their messages still reach Activity.')
	).toBeVisible();
	await expect(messages(page).filter({ hasText: 'Ready for review: create-org' })).toHaveCount(0);
	const muted = page.getByTestId('muted-row');
	await expect(muted).toContainText('1 muted message');
	await muted.getByRole('button').click();
	await expect(muted).toContainText('Ready for review: create-org');
	// Back to everything, then a digest from the Members panel.
	await page.getByRole('button', { name: 'Members' }).click();
	await page
		.getByTestId('member-row')
		.filter({ hasText: 'Sam Okafor' })
		.getByTestId('mute-select')
		.selectOption('digest');
	await expect(page.getByTestId('digest-row')).toContainText(
		'Daily digest · Sam Okafor · 1 update'
	);
});

test('CHAT-12: a conversation between agents is read-only for the owner', async ({ page }) => {
	await open(page);
	await side(page)
		.getByTestId('as-owner')
		.getByTestId('conversation-link')
		.filter({ hasText: 'Sam Okafor, Noor Haddad' })
		.click();
	await expect(page.getByTestId('reading-as-owner')).toHaveText(
		"You're reading as the owner. This conversation is read-only."
	);
	await expect(page.getByTestId('composer-note')).toHaveText(
		'Reading as the owner. To weigh in, message Sam Okafor.'
	);
	await expect(composer(page)).toHaveCount(0);
	await shot(page, 'chat-as-owner');
});

test('CHAT-13: search filters by text, member, channel, kind and date', async ({ page }) => {
	await open(page);
	const search = page.getByTestId('chat-search');
	const count = page.getByTestId('search-count');
	await search.fill('release notes');
	await expect(count).toContainText('1 result');
	await search.fill('from:@kai');
	await expect(count).toContainText('results');
	await expect(page.getByTestId('search-help')).toContainText('from:@name');
	await search.fill('has:ask');
	await expect(count).toContainText('results');
	await search.fill('in:#release-v1 from:@kai');
	await expect(count).toContainText('2 results');
	await search.fill('kind:announcement');
	await expect(count).toContainText('results');
	await search.fill('before:2020-01-01');
	await expect(page.getByTestId('no-results')).toBeVisible();
	await shot(page, 'chat-search-empty');
	await search.fill('from:@kai has:ask');
	await shot(page, 'chat-search');
	await page
		.getByTestId('message')
		.first()
		.getByRole('button', { name: 'Open in conversation' })
		.click();
	await expect(search).toHaveValue('');
	await expect(title(page)).toHaveText('#atlas');
});

test('CHAT-14: Markdown renders safely, item numbers are chips, long messages collapse, and @ completes a name to its handle', async ({
	page
}) => {
	await open(page);
	await go(page, 'general');
	const ready = messages(page).filter({ hasText: 'Ready for review: create-org' });
	await expect(ready.locator('strong')).toHaveText('create-org');
	await expect(ready.locator('code')).toHaveText('9c1e0b2');
	await expect(ready.getByTestId('item-chip')).toHaveText('ac-T-17');
	await expect(ready.getByRole('link', { name: 'the branch' })).toHaveAttribute(
		'href',
		'https://example.com/branch/create-org'
	);
	await expect(ready.getByRole('link', { name: 'the branch' })).toHaveAttribute(
		'rel',
		'noopener noreferrer'
	);
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

test('CHAT-15: the link an Activity row uses (/chat?c=…&m=…) opens the message in Chat', async ({
	page
}) => {
	await open(page);
	await go(page, 'atlas');
	const id = await messages(page).filter({ hasText: 'Publish the 0.2' }).getAttribute('data-id');
	await visit(page, `/chat?c=c-project-atlas&m=${id}`);
	await expect(title(page)).toHaveText('#atlas');
	await expect(messages(page).filter({ hasText: 'Publish the 0.2' })).toBeInViewport();
	await visit(page, '/chat?c=c-dm-sam');
	await expect(title(page)).toHaveText('Sam Okafor');
});

test('CHAT-17: a bridged message shows its platform, and asks to the owner and approvals are read-only on a bridge', async ({
	page
}) => {
	await open(page);
	await go(page, 'general');
	const bridged = messages(page).filter({ hasText: 'Can someone link the Atlas plan' });
	await expect(bridged).toContainText('Leo Brandt');
	await expect(bridged.getByTestId('bridge-badge')).toHaveText('Slack');
	await visit(page, '/chat?surface=bridge&c=c-project-atlas');
	await expect(page.getByTestId('bridge-preview')).toBeVisible();
	const card = page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
	await expect(card.getByTestId('app-only')).toContainText('Open in Agentic Org');
	await expect(card.getByRole('button', { name: 'Yes, block' })).toHaveCount(0);
	await expect(
		page.getByTestId('ask-card').filter({ hasText: 'Publish the 0.2' }).getByTestId('app-only')
	).toBeVisible();
	// Inside a thread too, and the composer is off.
	await go(page, 'release-v1');
	await page.getByTestId('replies-link').first().click();
	await expect(page.getByTestId('thread-panel').getByTestId('composer')).toHaveCount(0);
	await shot(page, 'chat-bridge-preview');
});

test(
	'states: the office is not running, a message that fails reconciles via Check status and Retry, and an empty organization has a welcome card',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page, '?chat=offline');
		await go(page, 'general');
		await expect(page.getByTestId('office-down')).toBeVisible();
		await expect(page.getByTestId('composer-note')).toHaveText(
			"The office isn't running. Restart it from Offices."
		);
		await expect(composer(page)).toHaveCount(0);
		await shot(page, 'chat-office-offline');

		await visit(page, '/chat?chat=fail');
		await go(page, 'general');
		await send(page, 'Will this arrive?');
		await expect(page.getByTestId('not-confirmed')).toContainText('Delivery not confirmed');
		await page.getByRole('button', { name: 'Check status' }).click();
		await expect(page.getByTestId('not-delivered')).toContainText(
			'Not delivered: the office has no record of it. Retry sends it once.'
		);
		await shot(page, 'chat-delivery-failed');
		await page.getByRole('button', { name: 'Retry' }).click();
		await expect(page.getByTestId('not-delivered')).toHaveCount(0);
		await expect(messages(page).filter({ hasText: 'Will this arrive?' })).toHaveCount(1);
		await shot(page, 'chat-delivery-retried');

		await visit(page, '/chat?org=empty');
		await expect(title(page)).toHaveText('#general');
		await expect(page.getByTestId('welcome-card')).toContainText('Agents and people talk here.');
		await shot(page, 'chat-empty');
	}
);

test('Step 8: multi-question 4-step QuestionForm wizard in #beacon', async ({ page }) => {
	await open(page);
	await go(page, 'beacon');
	const card = page.getByTestId('ask-card').filter({ hasText: 'Beacon first-run flow' });
	const form = card.getByTestId('question-form');
	await expect(form.getByTestId('question-step')).toHaveText('Question 1 of 4');
	await form.getByRole('button', { name: 'Next' }).click();
	await expect(form.getByTestId('question-step')).toHaveText('Question 2 of 4');
	await form.getByRole('button', { name: 'Next' }).click();
	await expect(form.getByTestId('question-step')).toHaveText('Question 3 of 4');
	await form.getByLabel('Projects').check();
	await form.getByLabel('Teams').check();
	await form.getByRole('button', { name: 'Next' }).click();
	await expect(form.getByTestId('question-step')).toHaveText('Question 4 of 4');
	await form.getByRole('textbox').fill('Keep it under two minutes.');
	await form.getByTestId('send-answers').click();
	await expect(card.getByTestId('answered')).toContainText('Answered by you');
	await expect(card.getByTestId('answered')).toContainText('4 of 4 answered');
});

test('Step 8: HandoffCard Hand back… requires a reason, and AskCard Withdraw… closes own ask', async ({
	page
}) => {
	await open(page);
	await go(page, 'Noor Haddad');
	const handoff = page.getByTestId('handoff-card').filter({ hasText: 'Beacon sign-up page' });
	await expect(handoff.getByTestId('linked-ref')).toContainText('T-B37');
	await handoff.getByRole('button', { name: 'Hand back…' }).click();
	const submitHandBack = handoff.getByRole('button', { name: 'Hand back to Noor Haddad' });
	await expect(submitHandBack).toBeDisabled();
	await handoff.getByLabel('Why do you hand it back?').fill('Needs the legal footer first.');
	await submitHandBack.click();
	await expect(handoff.getByTestId('answered')).toContainText('Handed back by you');
	await expect(handoff.getByTestId('answered')).toContainText('Needs the legal footer first.');

	await go(page, 'Sam Okafor');
	const ownAsk = page.getByTestId('ask-card').filter({ hasText: 'beta tag tonight' });
	await ownAsk.getByRole('button', { name: 'Withdraw…' }).click();
	await ownAsk.getByLabel('Why withdraw it? (optional)').fill('Holding until Monday.');
	await ownAsk.getByRole('button', { name: 'Withdraw', exact: true }).click();
	await expect(ownAsk.getByTestId('answered')).toContainText('Withdrawn by you');
	await expect(ownAsk.getByTestId('answered')).toContainText('Holding until Monday.');
});

test('Step 8: Browse channels joins open channel, requests invite, and hides restricted', async ({
	page
}) => {
	await open(page);
	await side(page).getByTestId('browse-channels').click();
	const dialog = page.getByRole('dialog', { name: 'Browse channels' });
	await expect(dialog).toBeVisible();
	await expect(dialog.getByTestId('browse-list')).not.toContainText('security-review');
	const inviteRow = dialog.getByTestId('browse-row').filter({ hasText: '#incident-drills' });
	await inviteRow.getByRole('button', { name: 'Request invitation' }).click();
	await expect(inviteRow).toContainText('Invitation requested');
	const openRow = dialog.getByTestId('browse-row').filter({ hasText: '#design-crit' });
	await openRow.getByRole('button', { name: 'Join' }).click();
	await expect(dialog).toBeHidden();
	await expect(title(page)).toHaveText('#design-crit');
});

test('Step 8: Pins panel shows pins, jumps to message, and unpins; System notice refuses replies', async ({
	page
}) => {
	await open(page);
	// System notice in #general.
	const notice = messages(page).filter({ hasText: 'Quinn Adeyemi joined the organization' });
	await expect(notice).toContainText('System');
	await expect(notice.getByTestId('system-avatar')).toBeVisible();
	await notice.hover();
	await notice.getByTestId('message-menu').click();
	await expect(
		page.getByRole('menuitem', { name: "System messages can't be replied to." })
	).toBeDisabled();
	await page.keyboard.press('Escape');

	// Pins panel in #general.
	const pinsButton = page.getByTestId('pins-button');
	await expect(pinsButton.getByTestId('pins-count')).toHaveText('1');
	await pinsButton.click();
	const pinsPanel = page.getByTestId('pins-panel');
	await expect(pinsPanel).toBeVisible();
	await expect(pinsPanel.getByTestId('pin-row')).toHaveCount(1);
	await pinsPanel.getByRole('button', { name: 'Show in conversation' }).click();
	await pinsPanel.getByRole('button', { name: 'Unpin' }).click();
	await expect(pinsPanel).toContainText('Nothing is pinned here.');
});

test('Step 8: ?chat=lost reconciles delivered send, ?chat=hr-down shows HR guidance, and /chat?v=needs opens /inbox', async ({
	page
}) => {
	await open(page, '?chat=lost');
	await send(page, 'Lost response but delivered');
	await expect(page.getByTestId('not-confirmed')).toBeVisible();
	await page.getByRole('button', { name: 'Check status' }).click();
	await expect(page.getByTestId('not-confirmed')).toHaveCount(0);
	await expect(messages(page).filter({ hasText: 'Lost response but delivered' })).toHaveCount(1);

	await visit(page, '/chat?chat=hr-down');
	await composer(page).fill('Hey @hr please help');
	await expect(page.getByTestId('composer-note')).toContainText(
		"HR lead can't run right now, so it won't answer yet."
	);

	await visit(page, '/chat?v=needs');
	await expect(page).toHaveURL(/\/inbox$/);
});

// Hold the existing organization read used by Chat's snapshot, without a production test hook.
type HeldChatRead = { started: Promise<void>; release: () => void };

for (const fail of [false, true]) {
	test(`CHAT-1: the initial snapshot shows loading, then ${fail ? 'its error with Retry' : 'the conversation list'}`, async ({
		page
	}) => {
		await open(page);
		await page
			.getByRole('navigation', { name: 'Main' })
			.getByRole('link', { name: 'Teams', exact: true })
			.click();
		await expect(page).toHaveURL(/\/teams$/);
		await page.evaluate(async (fail) => {
			// Share the singleton composed into the UI by the explicit e2e resolver.
			const modulePath = '/e2e/fakes/org-api.ts';
			const { orgApi } = await import(modulePath);
			const view = orgApi.view.bind(orgApi);
			let started!: () => void;
			let release!: () => void;
			const began = new Promise<void>((resolve) => (started = resolve));
			const gate = new Promise<void>((resolve) => (release = resolve));
			let reads = 0;
			orgApi.view = async (organizationId: string) => {
				// The route load obtains its organization first; hold the next read, Chat's snapshot.
				if (++reads === 1) return view(organizationId);
				orgApi.view = view;
				started();
				await gate;
				if (fail) throw { source: 'api', code: 'unavailable', message: 'The office went away.' };
				return view(organizationId);
			};
			(window as unknown as { __heldChatRead: HeldChatRead }).__heldChatRead = {
				started: began,
				release
			};
		}, fail);
		// Client navigation retains the same dev-module instance as Chat's snapshot adapter.
		await page
			.getByRole('navigation', { name: 'Main' })
			.getByRole('link', { name: 'Chat', exact: true })
			.click();
		await page.evaluate(
			() => (window as unknown as { __heldChatRead: HeldChatRead }).__heldChatRead.started
		);
		await expect(page.getByRole('status').filter({ hasText: /\S/ })).toHaveText('Loading chat…');
		await shot(page, 'chat-snapshot-loading', { settle: false });
		await page.evaluate(() =>
			(window as unknown as { __heldChatRead: HeldChatRead }).__heldChatRead.release()
		);
		if (fail) {
			await expect(page.getByRole('alert')).toHaveText('The office went away.');
			await expect(side(page)).toHaveCount(0);
			await page.getByTestId('chat-retry').click();
			await expect(side(page)).toBeVisible();
			await expect(title(page)).toHaveText('#general');
		} else {
			await expect(side(page)).toBeVisible();
			await expect(title(page)).toHaveText('#general');
		}
		await shot(page, fail ? 'chat-snapshot-error' : 'chat-snapshot-loaded');
	});
}
