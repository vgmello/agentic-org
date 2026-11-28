import { describe, expect, it, vi } from 'vitest';
import { createSessionController, type SessionState } from '../auth/session-controller';
import type { AccessView, EstablishedSession, SessionView } from '../auth/session-gateway';
import type { SessionRecord, SessionStore } from '../auth/session-record';
import { gatewayStatusCode, unwrapGatewayResult } from './gateway-errors';
import { createJsonHttp } from './json-http';
import { API_VERSION, CONTRACT_REVISION } from './wire';
import {
	ATTACHMENT_DOCUMENT_MAX_BYTES,
	ATTACHMENT_IMAGE_MAX_BYTES,
	createAttachmentsClient,
	normalizeAttachmentMime,
	validateAttachmentFile,
	validateAttachmentFilename
} from './attachments';
import type { AttachmentsRuntime } from './attachments-runtime';

function fileStub(name: string, type: string, size: number): File {
	const f = new File([new Uint8Array(Math.min(size, 16))], name, { type });
	Object.defineProperty(f, 'size', { value: size });
	return f;
}

describe('attachment filename, MIME allowlist, and size pre-checks', () => {
	it('accepts all allowlisted raster images, binary docs, and text docs within limits', () => {
		const validCases: Array<[string, string, number, string]> = [
			['shot.png', 'image/png', ATTACHMENT_IMAGE_MAX_BYTES, 'image/png'],
			['photo.jpg', 'image/jpeg', 1024, 'image/jpeg'],
			['pic.webp', 'image/webp', 1024, 'image/webp'],
			['anim.gif', 'image/gif', 1024, 'image/gif'],
			['spec.pdf', 'application/pdf', ATTACHMENT_DOCUMENT_MAX_BYTES, 'application/pdf'],
			[
				'doc.docx',
				'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
				1024,
				'application/vnd.openxmlformats-officedocument.wordprocessingml.document'
			],
			[
				'sheet.xlsx',
				'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
				1024,
				'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'
			],
			['README.md', '', 512, 'text/markdown'],
			['notes.md', 'text/x-markdown; charset=utf-8', 512, 'text/markdown'],
			['log.txt', 'text/plain; charset="utf-8"', 512, 'text/plain'],
			['data.csv', 'text/csv', 512, 'text/csv'],
			['config.json', 'application/json', 512, 'application/json']
		];
		for (const [name, type, size, expectedMime] of validCases) {
			const res = validateAttachmentFile(fileStub(name, type, size));
			expect(res, name).toMatchObject({ ok: true, filename: name, mimeType: expectedMime });
		}
	});

	it('rejects SVG, HTML, JS, octet-stream, MIME params on binaries, empty and oversized files', () => {
		const disallowed: Array<[string, string]> = [
			['vector.svg', 'image/svg+xml'],
			['page.html', 'text/html'],
			['script.js', 'application/javascript'],
			['script2.js', 'text/javascript'],
			['blob.bin', 'application/octet-stream'],
			['spoofed.png', 'image/svg+xml'],
			['spoofed.pdf', 'text/html'],
			['param-img.png', 'image/png; charset=utf-8'],
			['bad-charset.txt', 'text/plain; charset=iso-8859-1']
		];
		for (const [name, type] of disallowed) {
			expect(normalizeAttachmentMime(type, name), `${name} (${type})`).toBeUndefined();
			const check = validateAttachmentFile(fileStub(name, type, 100));
			expect(check).toMatchObject({ ok: false, code: 'unsupported' });
		}

		expect(validateAttachmentFile(fileStub('empty.png', 'image/png', 0))).toMatchObject({
			ok: false,
			code: 'invalid_request'
		});
		expect(
			validateAttachmentFile(fileStub('huge.png', 'image/png', ATTACHMENT_IMAGE_MAX_BYTES + 1))
		).toMatchObject({ ok: false, code: 'too_large' });
		expect(
			validateAttachmentFile(
				fileStub('huge.pdf', 'application/pdf', ATTACHMENT_DOCUMENT_MAX_BYTES + 1)
			)
		).toMatchObject({ ok: false, code: 'too_large' });
	});

	it('rejects invalid filenames (leading dot, slashes, control/bidi chars, whitespace, >255 UTF-8 bytes)', () => {
		const badNames = [
			'',
			'   ',
			' leading.txt',
			'trailing.txt ',
			'.env',
			'.hidden.png',
			'dir/file.png',
			'dir\\file.png',
			'ctrl\u0000.txt',
			'line\nbreak.txt',
			'del\u007f.txt',
			'spoof\u202Egpj.exe',
			'isolate\u2066name.txt',
			`${'é'.repeat(128)}.txt` // 256 + 4 = 260 UTF-8 bytes > 255
		];
		for (const name of badNames) {
			expect(validateAttachmentFilename(name), JSON.stringify(name)).toMatchObject({ ok: false });
		}
		expect(validateAttachmentFilename('café-report (v2).pdf')).toEqual({
			ok: true,
			filename: 'café-report (v2).pdf'
		});
	});
});

describe('gateway status mapping, PATCH transport, and attachments client', () => {
	it('maps 411/413/415/429 and unwraps 201 Created', () => {
		expect(gatewayStatusCode(411)).toBe('invalid_request');
		expect(gatewayStatusCode(413)).toBe('too_large');
		expect(gatewayStatusCode(415)).toBe('unsupported');
		expect(gatewayStatusCode(429)).toBe('rate_limited');
		expect(
			unwrapGatewayResult({ status: 201, body: { result: { upload_id: 'u1' } } }, 'err')
		).toEqual({
			upload_id: 'u1'
		});
	});

	it('supports PATCH in createJsonHttp', async () => {
		const send = vi.fn<typeof fetch>(async () => Response.json({ result: { ok: true } }));
		const http = createJsonHttp({
			origin: 'http://localhost:4173',
			fetch: send,
			timeoutMs: 1000,
			maxRequestBytes: 1024,
			maxResponseBytes: 1024
		});
		const reply = await http({
			path: '/api/v1/organizations/org-1/files/0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa',
			method: 'PATCH',
			proof: 'proof-token',
			body: { command_id: 'cmd-1', title: 'Updated' }
		});
		expect(reply.status).toBe(200);
		expect(send.mock.calls[0][1]?.method).toBe('PATCH');
	});

	it('rejects escaped query values in listFiles and validates updateFile fields', async () => {
		const runtime: AttachmentsRuntime = {
			read: vi.fn(async () => ({ status: 200, body: { result: { attachments: [] } } })),
			submit: vi.fn(async () => ({ status: 201, body: { result: {} } })),
			patch: vi.fn(async () => ({ status: 200, body: { result: {} } })),
			uploadAttachment: vi.fn(),
			downloadAttachment: vi.fn(),
			newOperationId: () => '0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa',
			hasSessionPair: () => true,
			onSessionEnded: () => () => {}
		};
		const client = createAttachmentsClient(() => runtime);

		await expect(client.listFiles('org-1', { cursor: 'bad%20cursor' })).rejects.toMatchObject({
			code: 'invalid_request'
		});
		await expect(client.listFiles('org-1', { target_id: 'a+b' })).rejects.toMatchObject({
			code: 'invalid_request'
		});
		for (const bad of ['a=b', 'a#b', 'a?b', 'a\u0007b', 'a\u0085b']) {
			await expect(client.listFiles('org-1', { target_id: bad })).rejects.toMatchObject({
				code: 'invalid_request'
			});
		}
		for (const badLimit of [0, -1, 201, 1.5, Number.NaN]) {
			await expect(client.listFiles('org-1', { limit: badLimit })).rejects.toMatchObject({
				code: 'invalid_request'
			});
		}
		await expect(
			client.registerFile('org-1', { target_kind: 'organization' })
		).rejects.toMatchObject({ code: 'invalid_request' });
		await expect(
			client.registerFile('org-1', {
				upload_id: 'u-1',
				source_attachment_id: 'a-1',
				target_kind: 'organization'
			})
		).rejects.toMatchObject({ code: 'invalid_request' });
		await expect(client.updateFile('org-1', 'file-1', {})).rejects.toMatchObject({
			code: 'invalid_request'
		});

		const submit = runtime.submit as ReturnType<typeof vi.fn>;
		submit.mockResolvedValueOnce({
			status: 201,
			body: {
				result: {
					attachment: {
						id: 'a-promoted',
						filename: 'spec.pdf',
						title: 'Spec',
						artifact_status: 'final',
						mime_type: 'application/pdf',
						byte_count: 10,
						uploaded_by_employee_id: 'emp-1',
						created_at: '2026-11-27T00:00:00.000Z',
						scope: 'shared',
						target_kind: 'organization'
					}
				}
			}
		});
		await client.registerFile('org-1', {
			upload_id: '   ',
			source_attachment_id: ' a-1 ',
			target_kind: 'organization'
		});
		const sentBody = submit.mock.calls.at(-1)?.[1] as Record<string, unknown>;
		expect(sentBody).not.toHaveProperty('upload_id');
		expect(sentBody.source_attachment_id).toBe('a-1');

		await client.listFiles('org-1', { scope: 'shared', mime_category: 'document', limit: 25 });
		expect(runtime.read).toHaveBeenCalledWith(
			'/api/v1/organizations/org-1/files?scope=shared&mime_category=document&limit=25',
			undefined
		);
	});
});

describe('session-binary auth asymmetry & header encoding', () => {
	const baseView: SessionView = {
		instance_id: 'hq',
		account: { id: 'acc-1', login: 'local' },
		admin: true,
		organizations: [],
		server_time_ms: 1000,
		idle_expires_at: '2026-12-01T00:00:00.000Z'
	};

	function makeController(opts: {
		authEnabled: boolean;
		stored?: SessionRecord;
		binaryFetch: typeof fetch;
	}) {
		let stored: SessionRecord | undefined = opts.stored;
		const store: SessionStore = {
			read: async () => stored,
			write: async (next) => {
				stored = next;
			},
			clear: async () => {
				stored = undefined;
			}
		};
		const states: SessionState[] = [];
		const accesses: AccessView[] = [];
		const ctrl = createSessionController({
			store,
			gateway: {
				version: async () => ({
					api_version: API_VERSION,
					contract_revision: CONTRACT_REVISION,
					capabilities: []
				}),
				access: async () => ({
					auth_enabled: opts.authEnabled,
					listen_scheme: 'http_loopback'
				}),
				getLocal: async () => baseView,
				get: async () => baseView,
				signIn: async () => ({ ...baseView, proof: 'p1' }) as EstablishedSession,
				register: async () => ({ ...baseView, proof: 'p1' }) as EstablishedSession,
				refresh: async () => ({ ...baseView, proof: 'p2' }) as EstablishedSession,
				signOut: async () => ({})
			},
			http: vi.fn(async () => ({ status: 200, body: { result: {} } })),
			authlessHttp: vi.fn(async () => ({ status: 200, body: { result: {} } })),
			fetch: opts.binaryFetch,
			locks: {
				request: (async (_n: string, b: unknown, c?: unknown) => {
					const cb = (typeof b === 'function' ? b : c) as (lock: Lock | null) => Promise<unknown>;
					return cb(null);
				}) as LockManager['request']
			},
			channel: {
				postMessage: () => {},
				addEventListener: () => {},
				removeEventListener: () => {}
			},
			now: () => 1000,
			changed: (next) => states.push(next),
			accessChanged: (next) => accesses.push(next)
		});
		return {
			ctrl,
			states,
			accesses,
			setStored: (next: SessionRecord | undefined) => (stored = next)
		};
	}

	it('fails closed in local-authless mode WITHOUT calling fetch, ending session, or flipping auth_enabled', async () => {
		const binaryFetch = vi.fn<typeof fetch>();
		const { ctrl, states, accesses } = makeController({ authEnabled: false, binaryFetch });
		await ctrl.boot();
		expect(states.at(-1)).toMatchObject({ kind: 'signed-in', authless: true });
		expect(ctrl.access().auth_enabled).toBe(false);

		const stateCountBefore = states.length;
		const accessCountBefore = accesses.length;

		await expect(
			ctrl.uploadAttachment({
				path: '/api/v1/organizations/org-1/attachments',
				body: new Blob(['hello'], { type: 'text/plain' }),
				contentType: 'text/plain',
				filename: 'hello.txt'
			})
		).rejects.toMatchObject({ source: 'api', code: 'not_authorized' });

		await expect(
			ctrl.downloadAttachment({
				path: '/api/v1/organizations/org-1/attachments/0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa'
			})
		).rejects.toMatchObject({ source: 'api', code: 'not_authorized' });

		expect(binaryFetch).not.toHaveBeenCalled();
		expect(states).toHaveLength(stateCountBefore);
		expect(accesses).toHaveLength(accessCountBefore);
		expect(ctrl.access().auth_enabled).toBe(false);
	});

	it('encodes X-Attachment-Filename via encodeURIComponent, retries once on rotated 401, and handles 304', async () => {
		const record: SessionRecord = {
			instance_id: 'hq',
			account_id: 'acc-1',
			login: 'local',
			proof: 'a'.repeat(64),
			rotated_at_ms: 1000
		};
		const rotated: SessionRecord = { ...record, proof: 'b'.repeat(64), rotated_at_ms: 2000 };
		let callCount = 0;
		const binaryFetch = vi.fn<typeof fetch>(async (_input, init) => {
			callCount++;
			const headers = new Headers(init?.headers);
			if (callCount === 1) {
				expect(headers.get('x-org-session-proof')).toBe(record.proof);
				expect(headers.get('x-attachment-filename')).toBe(encodeURIComponent('café spec (1).pdf'));
				expect(headers.get('content-type')).toBe('application/pdf');
				return Response.json({ error: { code: 'unauthenticated' } }, { status: 401 });
			}
			if (callCount === 2) {
				expect(headers.get('x-org-session-proof')).toBe(rotated.proof);
				return Response.json(
					{ result: { upload_id: '0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa' } },
					{ status: 201 }
				);
			}
			expect(headers.get('if-none-match')).toBe('"deadbeef"');
			return new Response(null, { status: 304, headers: { ETag: '"deadbeef"' } });
		});

		const { ctrl, setStored } = makeController({
			authEnabled: true,
			stored: record,
			binaryFetch
		});
		await ctrl.boot();
		// Simulate another tab rotating the proof before the 401 retry reads IndexedDB.
		setStored(rotated);

		const uploadReply = await ctrl.uploadAttachment({
			path: '/api/v1/organizations/org-1/attachments',
			body: new Blob(['%PDF-1.4'], { type: 'application/pdf' }),
			contentType: 'application/pdf',
			filename: 'café spec (1).pdf'
		});
		expect(uploadReply.status).toBe(201);

		const downloadReply = await ctrl.downloadAttachment({
			path: '/api/v1/organizations/org-1/attachments/0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa',
			ifNoneMatch: '"deadbeef"'
		});
		expect(downloadReply).toEqual({ status: 304, etag: '"deadbeef"' });
	});
});
