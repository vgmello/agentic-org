import { describe, expect, it, vi } from 'vitest';
import {
	createHttpTessApi,
	decodeUiBlock,
	parseTessSseEvent,
	splitCanonicalRefSegments,
	type TessSessionTransport
} from './api';
import type { TessStreamEvent } from './types';

function sseStream(chunks: string[]): ReadableStream<Uint8Array> {
	const enc = new TextEncoder();
	return new ReadableStream({
		start(controller) {
			for (const chunk of chunks) controller.enqueue(enc.encode(chunk));
			controller.close();
		}
	});
}

describe('Tess HTTP + SSE API client (doc-97 §5.C)', () => {
	it('decodes streaming ask responses and parses SSE tool.progress, text.delta, ui.block, and turn.completed events', async () => {
		const session: TessSessionTransport = {
			submit: vi.fn().mockResolvedValueOnce({
				status: 200,
				body: {
					ok: true,
					result: {
						turn_id: 'turn-42',
						status: 'streaming',
						stream_url: '/api/v1/organizations/org-1/tess/turns/turn-42/stream'
					}
				}
			}),
			openSseStream: vi
				.fn()
				.mockResolvedValueOnce(
					new Response(
						sseStream([
							': heartbeat\n\n',
							'event: turn.started\ndata: {"turn_id":"turn-42"}\n\n',
							'event: tool.progress\ndata: {"id":"c1","label":"Checking projects…","state":"running","started_at":100}\n\n',
							'event: tool.progress\ndata: {"id":"c1","label":"Checked 3 projects","state":"done","duration_ms":840}\n\n',
							'event: text.delta\ndata: {"delta":"All 3 projects look healthy. See T-12."}\n\n',
							'event: ui.block\ndata: {"kind":"action_proposal","id":"sug-1","title":"Create project","risk":"reversible","cta_label":"Create project","parameters":[{"label":"Name","value":"iOS App"}],"edit_href":"/projects?new"}\n\n',
							'event: turn.completed\ndata: {"turn_id":"turn-42","follow_ups":["Show blocked tasks","Check budget"]}\n\n'
						]),
						{ status: 200, headers: { 'Content-Type': 'text/event-stream' } }
					)
				)
		};

		const api = createHttpTessApi({ session });
		const askRes = await api.ask('org-1', { prompt: 'Create an iOS project' });
		expect(askRes).toEqual({
			kind: 'streaming',
			turnId: 'turn-42',
			streamUrl: '/api/v1/organizations/org-1/tess/turns/turn-42/stream'
		});

		const received: TessStreamEvent[] = [];
		let doneCalled = false;
		await api.streamTurn(
			'org-1',
			'turn-42',
			'/api/v1/organizations/org-1/tess/turns/turn-42/stream',
			{
				onEvent: (ev) => received.push(ev),
				onDone: () => {
					doneCalled = true;
				}
			}
		);

		expect(doneCalled).toBe(true);
		expect(received.map((e) => e.type)).toEqual([
			'turn.started',
			'tool.progress',
			'tool.progress',
			'text.delta',
			'ui.block',
			'turn.completed'
		]);
		const blockEv = received[4];
		expect(blockEv.type === 'ui.block' && blockEv.block.kind === 'action_proposal').toBe(true);
	});

	it('returns unavailable cleanly on 404/501 when TASK-80.4 is not yet deployed', async () => {
		const session: TessSessionTransport = {
			submit: vi.fn().mockResolvedValueOnce({ status: 404, body: undefined }),
			openSseStream: vi.fn()
		};
		const api = createHttpTessApi({ session });
		const res = await api.ask('org-1', { prompt: 'Hello' });
		expect(res.kind).toBe('unavailable');
	});

	it('handles two-phase applyProposal receipts and 409 stale revision conflicts', async () => {
		const session: TessSessionTransport = {
			submit: vi
				.fn()
				.mockResolvedValueOnce({
					status: 200,
					body: {
						ok: true,
						result: {
							receipt: {
								summary: 'Created project iOS App (P-2).',
								entity_href: '/projects/p-2',
								entity_label: 'Open iOS App'
							}
						}
					}
				})
				.mockResolvedValueOnce({
					status: 409,
					body: {
						ok: false,
						error: { code: 'conflict', message: 'Record changed since preview.' }
					}
				}),
			openSseStream: vi.fn()
		};

		const api = createHttpTessApi({ session });
		const okRes = await api.applyProposal('org-1', 'sug-1', 'rev-3');
		expect(okRes).toEqual({
			ok: true,
			receipt: {
				summary: 'Created project iOS App (P-2).',
				entityHref: '/projects/p-2',
				entityLabel: 'Open iOS App',
				appliedAt: undefined
			}
		});

		const conflictRes = await api.applyProposal('org-1', 'sug-2', 'rev-1');
		expect(conflictRes).toEqual({
			ok: false,
			code: 'conflict',
			message: 'Record changed since preview.'
		});
	});

	it('auto-links canonical refs safely (including office-prefixed T-B37) and rejects unsafe hrefs', () => {
		expect(splitCanonicalRefSegments('Review T-12, T-B37, and BUG-4 now.')).toEqual([
			{ text: 'Review ' },
			{ text: 'T-12', href: '/work/T-12' },
			{ text: ', ' },
			{ text: 'T-B37', href: '/work/T-B37' },
			{ text: ', and ' },
			{ text: 'BUG-4', href: '/work/BUG-4' },
			{ text: ' now.' }
		]);

		const block = decodeUiBlock({
			kind: 'action_proposal',
			id: 'sug-x',
			title: 'Assign task',
			edit_href: 'https://evil.example/phish'
		});
		expect(block?.kind === 'action_proposal' && block.proposal.editHref).toBeUndefined();
		const ctrlBlock = decodeUiBlock({
			kind: 'action_proposal',
			id: 'sug-y',
			title: 'Assign task',
			edit_href: '/work/T-1\n/evil'
		});
		expect(ctrlBlock?.kind === 'action_proposal' && ctrlBlock.proposal.editHref).toBeUndefined();
		expect(parseTessSseEvent('text.delta', '{"delta":"Hi"}')).toEqual({
			type: 'text.delta',
			delta: 'Hi'
		});
	});

	it('preserves SSE events when CRLF splits across chunks and keeps indented data whitespace', async () => {
		const encoder = new TextEncoder();
		const body = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(encoder.encode('event: text.delta\r'));
				controller.enqueue(encoder.encode('\ndata:  {"delta":"  indented"}\r\n\r\n'));
				controller.close();
			}
		});
		const openSseStream = vi
			.fn()
			.mockResolvedValueOnce(
				new Response(body, { status: 200, headers: { 'Content-Type': 'text/event-stream' } })
			);
		const api = createHttpTessApi({
			session: { submit: vi.fn(), openSseStream }
		});
		const events: TessStreamEvent[] = [];
		await api.streamTurn('org-1', 'turn-crlf', '/ignored/evil/path', {
			onEvent: (ev) => events.push(ev)
		});
		expect(openSseStream).toHaveBeenCalledWith(
			'/api/v1/organizations/org-1/tess/turns/turn-crlf/stream',
			undefined
		);
		expect(events).toEqual([{ type: 'text.delta', delta: '  indented' }]);
	});
});
