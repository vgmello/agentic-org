import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import type { ComponentProps } from 'svelte';
import MessageRow from './components/MessageRow.svelte';
import { ctxAs, NOW } from './testkit';

const ctx = ctxAs();
const message = { id: 'root', from: 'sam', conversation_id: 'c-general', kind: 'reply' as const, body: 'Plain message.', created_at: NOW };
const base: ComponentProps<typeof MessageRow> = { message, ctx, now: NOW, rowData: { replies: [], mode: 'none' } };
const html = (props: Partial<typeof base> = {}) => render(MessageRow, { props: { ...base, ...props } }).body;

describe('MEM-2 cold row presentation', () => {
	for (const [inThread, withThread] of [[false, false], [true, true], [false, true]] as const) {
		it(`offers Reply only outside a thread with a handler (${inThread}/${withThread})`, () => {
			const output = html({ inThread, active: true, onthread: withThread ? () => undefined : undefined });
			expect(output.includes('aria-label="Reply in thread"')).toBe(!inThread && withThread);
		});
	}
	for (const delivery of ['sending', 'failed'] as const) {
		it(`retains ${delivery} feedback without offering row actions`, () => {
			const output = html({ delivery: { status: delivery }, active: true, onthread: () => undefined });
			expect(output).not.toContain('data-testid="message-menu"');
			expect(output).not.toContain('role="toolbar"');
			expect(output).toContain(delivery === 'sending' ? 'Sending…' : 'Not delivered:');
		});
	}
	it('retains read-only rows without actions even when active', () => {
		const output = html({ readOnly: true, active: true });
		expect(output).not.toContain('data-testid="message-menu"');
		expect(output).not.toContain('role="toolbar"');
	});
	const metadata: [string, Partial<typeof base>, string][] = [
		['kind', { message: { ...message, kind: 'announcement' } }, 'Announcement'],
		['bridge', { message: { ...message, bridge: 'slack' } }, 'data-testid="bridge-badge"'],
		['channel reply', { message: { ...message, also_in_channel: true } }, 'replied to a thread'],
		['pin', { message: { ...message, pinned: true } }, 'title="Pinned"'],
		['location', { where: '#elsewhere' }, 'in #elsewhere'],
		['mute', { rowData: { ...base.rowData, mode: 'muted' } }, '>muted</span>'],
		['digest', { rowData: { ...base.rowData, mode: 'digest' } }, '>digest</span>']
	];
	for (const [name, props, expected] of metadata) {
		it(`retains ${name} metadata when it is the only secondary field`, () => {
			expect(html(props)).toContain(expected);
			expect(html()).not.toContain(expected);
		});
	}
	const details: [string, Partial<typeof base>, string][] = [
		['attachment', { message: { ...message, attachments: ['design.txt'] } }, 'design.txt'],
		['reaction', { message: { ...message, reactions: { seen: ['owner'] } } }, 'aria-label="Seen, 1"'],
		['replies', { rowData: { ...base.rowData, replies: [{ ...message, id: 'reply', thread_root_id: message.id }] }, onthread: () => undefined }, 'data-testid="replies-link"'],
		['navigation', { onopen: () => undefined }, 'Open in conversation'],
		['sending', { delivery: { status: 'sending' } }, 'Sending…'],
		['failure', { delivery: { status: 'failed', error: 'Retry later' } }, 'Retry later']
	];
	for (const [name, props, expected] of details) {
		it(`retains ${name} when it is the only detail under the body`, () => {
			expect(html(props)).toContain(expected);
			expect(html()).not.toContain(expected);
		});
	}
});
