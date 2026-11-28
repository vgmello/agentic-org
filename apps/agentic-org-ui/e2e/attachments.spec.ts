import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { deflateSync } from 'node:zlib';
import { enterMockedOrganization } from './mock-organization';
import { visit } from './ready';

// Valid 1x1 red PNG (67 bytes, valid IHDR 1x1 so both browser decode & PNG dimension sniffer succeed).
const PNG_1X1_BASE64 =
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC';

function buildCrcTable(): Uint32Array {
	const table = new Uint32Array(256);
	for (let n = 0; n < 256; n++) {
		let c = n;
		for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
		table[n] = c >>> 0;
	}
	return table;
}

const PNG_CRC_TABLE = buildCrcTable();

function pngChunk(type: string, data: Buffer): Buffer {
	const out = Buffer.alloc(12 + data.length);
	out.writeUInt32BE(data.length, 0);
	out.write(type, 4, 4, 'ascii');
	data.copy(out, 8);
	let crc = 0xffffffff;
	for (let i = 4; i < 8 + data.length; i++)
		crc = PNG_CRC_TABLE[(crc ^ out[i]) & 0xff] ^ (crc >>> 8);
	out.writeUInt32BE((crc ^ 0xffffffff) >>> 0, 8 + data.length);
	return out;
}

function makeSolidPng(width: number, height: number): Buffer {
	const ihdr = Buffer.alloc(13);
	ihdr.writeUInt32BE(width, 0);
	ihdr.writeUInt32BE(height, 4);
	ihdr[8] = 8; // bit depth
	ihdr[9] = 2; // RGB
	const rowLen = 1 + width * 3;
	const row = Buffer.alloc(rowLen);
	for (let x = 0; x < width; x++) row[1 + x * 3] = 200;
	const raw = Buffer.concat(Array.from({ length: height }, () => row));
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		pngChunk('IHDR', ihdr),
		pngChunk('IDAT', deflateSync(raw)),
		pngChunk('IEND', Buffer.alloc(0))
	]);
}

const WIDE_PNG = makeSolidPng(2400, 40);

const SAMPLE_PDF = Buffer.from(
	'%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [] /Count 0 >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n',
	'utf8'
);

async function openChat(page: Page) {
	await enterMockedOrganization(page);
	await expect(page).toHaveURL(/\/teams\/members$/);
	await visit(page, '/chat?c=c-general');
	await expect(page.getByTestId('conversation-title')).toHaveText('#general');
}

test(
	'TASK-98.5 Slice 2A: attach/paste PNG, send attachment-only message, open Lightbox, Esc restores focus, and download PDF',
	{ tag: '@webkit' },
	async ({ page }) => {
		const consoleErrors: string[] = [];
		const pageErrors: string[] = [];
		const cspViolations: string[] = [];
		page.on('console', (msg) => {
			if (msg.type() === 'error') consoleErrors.push(msg.text());
		});
		page.on('pageerror', (err) => {
			pageErrors.push(err.message);
		});
		await page.addInitScript(() => {
			window.addEventListener('securitypolicyviolation', (event) => {
				(window as unknown as { __cspViolations?: string[] }).__cspViolations ??= [];
				(window as unknown as { __cspViolations: string[] }).__cspViolations.push(
					`${event.effectiveDirective}:${event.blockedURI}`
				);
			});
		});

		await openChat(page);
		// Clear initial Vite dev mock-mode /api/v1/version probe 404s before exercising attachments.
		consoleErrors.length = 0;

		// 1. Attach wide PNG image -> staged preview renders immediately with blob: URL + vision note.
		await page.getByLabel('Attach files').setInputFiles({
			name: 'diagram.png',
			mimeType: 'image/png',
			buffer: WIDE_PNG
		});

		const staged = page.getByTestId('staged-attachment');
		await expect(staged).toHaveCount(1);
		await expect(staged).toHaveAttribute('data-status', 'ready');
		await expect(staged.locator('img')).toHaveAttribute('src', /^blob:/);
		await expect(page.getByTestId('vision-capability-note')).toBeVisible();

		// 2. Attachment-only send (composer textarea stays completely empty).
		const sendBtn = page.getByRole('button', { name: /^Send/ });
		await expect(sendBtn).toBeEnabled();
		await sendBtn.click();

		// 3. Sent message renders <AttachmentImage> via blob: URL with real decoded pixels.
		const imageTile = page.getByTestId('attachment-image-tile').last();
		const imageTrigger = imageTile.getByTestId('attachment-image-trigger');
		await expect(imageTrigger).toBeVisible();
		const renderedImg = imageTile.locator('img');
		await expect(renderedImg).toHaveAttribute('src', /^blob:/);
		await expect
			.poll(() => renderedImg.evaluate((el: HTMLImageElement) => el.naturalWidth))
			.toBeGreaterThan(0);

		// 4. Click opens <ImageLightbox> (DialogContent), zoom toggles, and Escape returns focus to trigger.
		await imageTrigger.click();
		const lightbox = page.getByTestId('image-lightbox');
		await expect(lightbox).toBeVisible();
		await expect(lightbox.locator('img')).toHaveAttribute('src', /^blob:/);
		const zoomBtn = lightbox.getByRole('button', { name: 'Actual size', exact: true });
		await zoomBtn.click();
		await expect(
			lightbox.getByRole('button', { name: 'Fit to screen', exact: true })
		).toBeVisible();
		// Wide zoomed image must overflow and scroll horizontally instead of shrinking/clipping (N2).
		await expect
			.poll(() =>
				lightbox.getByTestId('lightbox-scroll').evaluate((el) => el.scrollWidth > el.clientWidth)
			)
			.toBe(true);

		await page.keyboard.press('Escape');
		await expect(lightbox).toBeHidden();
		await expect(imageTrigger).toBeFocused();

		// Reopening resets zoom state cleanly back to fit-to-screen (M2).
		await imageTrigger.click();
		await expect(lightbox).toBeVisible();
		await expect(lightbox.getByRole('button', { name: 'Actual size', exact: true })).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(lightbox).toBeHidden();
		await expect(imageTrigger).toBeFocused();

		// 5. Attach PDF document -> renders <DocumentAttachmentChip> with size, pages, and download button.
		await page.getByLabel('Attach files').setInputFiles({
			name: 'architecture-spec.pdf',
			mimeType: 'application/pdf',
			buffer: SAMPLE_PDF
		});
		await expect(page.getByTestId('staged-attachment')).toHaveAttribute('data-status', 'ready');
		await page.getByTestId('composer').fill('Spec attached');
		await page.getByRole('button', { name: /^Send/ }).click();

		const pdfChip = page
			.getByTestId('message')
			.filter({ hasText: 'Spec attached' })
			.getByTestId('document-attachment-chip');
		await expect(pdfChip).toContainText('architecture-spec.pdf');
		await expect(pdfChip).toContainText('PDF');
		await expect(pdfChip).toContainText('2 pp.');

		// Trigger authenticated download and ensure no error alert appears.
		const downloadBtn = pdfChip.getByTestId('download-attachment');
		await expect(downloadBtn).toBeEnabled();
		await downloadBtn.click();
		await expect(pdfChip.getByRole('alert')).toHaveCount(0);

		// 6. Clipboard paste of a PNG stages an immediate blob: preview, and removing it cleans up.
		await page.getByTestId('composer').evaluate((el, b64) => {
			const raw = atob(b64);
			const bytes = new Uint8Array(raw.length);
			for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
			const dt = new DataTransfer();
			dt.items.add(new File([bytes], 'pasted.png', { type: 'image/png' }));
			el.dispatchEvent(
				new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dt })
			);
		}, PNG_1X1_BASE64);
		const pastedStaged = page.getByTestId('staged-attachment');
		await expect(pastedStaged).toHaveCount(1);
		await expect(pastedStaged).toHaveAttribute('data-status', 'ready');
		await expect(pastedStaged.locator('img')).toHaveAttribute('src', /^blob:/);
		await page.getByRole('button', { name: 'Remove pasted.png', exact: true }).click();
		await expect(pastedStaged).toHaveCount(0);

		// 7. Disallowed file type (.svg) is rejected client-side with an accessible alert (never staged).
		await page.getByLabel('Attach files').setInputFiles({
			name: 'xss.svg',
			mimeType: 'image/svg+xml',
			buffer: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"></svg>', 'utf8')
		});
		await expect(page.getByTestId('staged-attachment')).toHaveCount(0);
		await expect(page.getByTestId('attachment-notice')).toContainText(/not accepted/i);

		// 8. Search with has:image and has:file filters cleanly.
		const search = page.getByTestId('chat-search');
		await search.fill('has:image');
		await expect(page.getByTestId('attachment-image-trigger')).toHaveCount(1);
		await search.fill('has:file');
		await expect(page.getByTestId('message').filter({ hasText: 'Spec attached' })).toBeVisible();

		const capturedViolations = await page.evaluate(
			() => (window as unknown as { __cspViolations?: string[] }).__cspViolations ?? []
		);
		cspViolations.push(...capturedViolations);
		expect(cspViolations).toEqual([]);
		expect(pageErrors).toEqual([]);
		expect(consoleErrors).toEqual([]);
	}
);

test(
	'TASK-98.5 Slice 2A H2: switching conversations mid-upload clears notice and marks outgoing upload retryable',
	{ tag: '@webkit' },
	async ({ page }) => {
		const pageErrors: string[] = [];
		page.on('pageerror', (err) => pageErrors.push(err.message));

		await openChat(page);

		// Trigger a validation notice in #general first.
		await page.getByLabel('Attach files').setInputFiles({
			name: 'bad.svg',
			mimeType: 'image/svg+xml',
			buffer: Buffer.from('<svg></svg>', 'utf8')
		});
		await expect(page.getByTestId('attachment-notice')).toBeVisible();

		// Hold attachment uploads in-flight so we can switch conversations mid-upload.
		await page.evaluate(async () => {
			const mod = await import('/e2e/fakes/attachments-runtime.ts');
			const runtime = mod.getAttachmentsRuntime() as {
				uploadAttachment: (...args: unknown[]) => Promise<unknown>;
				__origUpload?: (...args: unknown[]) => Promise<unknown>;
			};
			runtime.__origUpload = runtime.uploadAttachment;
			runtime.uploadAttachment = (req: { signal?: AbortSignal }) =>
				new Promise((_resolve, reject) => {
					req.signal?.addEventListener('abort', () =>
						reject(new DOMException('Aborted', 'AbortError'))
					);
				});
		});

		await page.getByLabel('Attach files').setInputFiles({
			name: 'in-flight.png',
			mimeType: 'image/png',
			buffer: Buffer.from(PNG_1X1_BASE64, 'base64')
		});
		const staged = page.getByTestId('staged-attachment');
		await expect(staged).toHaveAttribute('data-status', 'uploading');

		// Switch to #atlas: validation notice and #general's staged file must NOT leak into #atlas.
		await page.getByRole('button', { name: /^atlas\b/i }).click();
		await expect(page.getByTestId('conversation-title')).toHaveText('#atlas');
		await expect(page.getByTestId('attachment-notice')).toHaveCount(0);
		await expect(page.getByTestId('staged-attachment')).toHaveCount(0);

		// Restore uploads, switch back to #general: outgoing upload must be retryable error, NOT stuck uploading.
		await page.evaluate(async () => {
			const mod = await import('/e2e/fakes/attachments-runtime.ts');
			const runtime = mod.getAttachmentsRuntime() as {
				uploadAttachment: (...args: unknown[]) => Promise<unknown>;
				__origUpload?: (...args: unknown[]) => Promise<unknown>;
			};
			if (runtime.__origUpload) runtime.uploadAttachment = runtime.__origUpload;
		});
		await page.getByRole('button', { name: /^general\b/i }).click();
		await expect(page.getByTestId('conversation-title')).toHaveText('#general');
		await expect(staged).toHaveCount(1);
		await expect(staged).toHaveAttribute('data-status', 'error');
		await expect(staged.locator('img')).toHaveAttribute('src', /^blob:/);

		// Clicking Retry completes the upload cleanly.
		await page.getByRole('button', { name: 'Retry in-flight.png', exact: true }).click();
		await expect(staged).toHaveAttribute('data-status', 'ready');

		// N1: After a conversation switch, attaching [valid.png, bad.svg] sets notice WITHOUT aborting valid.png.
		await page.getByRole('button', { name: /^atlas\b/i }).click();
		await expect(page.getByTestId('conversation-title')).toHaveText('#atlas');
		await page.getByLabel('Attach files').setInputFiles([
			{
				name: 'mixed-ok.png',
				mimeType: 'image/png',
				buffer: Buffer.from(PNG_1X1_BASE64, 'base64')
			},
			{
				name: 'mixed-bad.svg',
				mimeType: 'image/svg+xml',
				buffer: Buffer.from('<svg></svg>', 'utf8')
			}
		]);
		await expect(page.getByTestId('attachment-notice')).toContainText(/not accepted/i);
		await expect(staged).toHaveCount(1);
		await expect(staged).toHaveAttribute('data-status', 'ready');
		expect(pageErrors).toEqual([]);
	}
);

test(
	'TASK-98.5 Slice 2A M1/L2: gallery error Retry has zero nested buttons (axe clean) and Lightbox arrows work with tooltip open',
	{ tag: '@webkit' },
	async ({ page }) => {
		const pageErrors: string[] = [];
		page.on('pageerror', (err) => pageErrors.push(err.message));

		await openChat(page);

		// Stage two PNGs so we can exercise both gallery error Retry (M1) and multi-image Lightbox nav (L2).
		await page.getByLabel('Attach files').setInputFiles([
			{
				name: 'first.png',
				mimeType: 'image/png',
				buffer: Buffer.from(PNG_1X1_BASE64, 'base64')
			},
			{
				name: 'second.png',
				mimeType: 'image/png',
				buffer: Buffer.from(PNG_1X1_BASE64, 'base64')
			}
		]);
		const staged = page.getByTestId('staged-attachment');
		await expect(staged).toHaveCount(2);
		await expect(staged.nth(0)).toHaveAttribute('data-status', 'ready');
		await expect(staged.nth(1)).toHaveAttribute('data-status', 'ready');

		// Fail the very first gallery image download once, then succeed on Retry.
		await page.evaluate(async () => {
			const mod = await import('/e2e/fakes/attachments-runtime.ts');
			const runtime = mod.getAttachmentsRuntime() as {
				downloadAttachment: (req: unknown) => Promise<unknown>;
			};
			const origDownload = runtime.downloadAttachment;
			let failedOnce = false;
			runtime.downloadAttachment = (req: unknown) => {
				if (!failedOnce) {
					failedOnce = true;
					return Promise.reject(new Error('Simulated transient download failure'));
				}
				return origDownload(req);
			};
		});

		await page.getByTestId('composer').fill('Two screenshots');
		await page.getByRole('button', { name: /^Send/ }).click();

		const grid = page
			.getByTestId('message')
			.filter({ hasText: 'Two screenshots' })
			.getByTestId('attachment-image-grid');
		await expect(grid.getByTestId('attachment-image-error')).toHaveCount(1);
		const retryBtn = grid.getByTestId('attachment-image-retry');
		await expect(retryBtn).toBeVisible();

		// M1: Zero nested <button> inside <button> and zero axe WCAG / nested-interactive violations.
		await expect(grid.locator('button button')).toHaveCount(0);
		const axeScan = await new AxeBuilder({ page })
			.include('[data-testid="attachment-image-grid"]')
			.withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
			.analyze();
		expect(axeScan.violations).toEqual([]);

		// Clicking gallery Retry recovers the image inline and MUST NOT open the lightbox.
		await retryBtn.click();
		await expect(page.getByTestId('image-lightbox')).toHaveCount(0);
		await expect(grid.getByTestId('attachment-image-error')).toHaveCount(0);
		await expect(grid.locator('img')).toHaveCount(2);
		await expect
			.poll(() =>
				grid
					.locator('img')
					.evaluateAll((imgs) => imgs.every((el) => (el as HTMLImageElement).naturalWidth > 0))
			)
			.toBe(true);
		const readyAxeScan = await new AxeBuilder({ page })
			.include('[data-testid="attachment-image-grid"]')
			.withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
			.analyze();
		expect(readyAxeScan.violations).toEqual([]);

		// L2: Open Lightbox, hover toolbar button so ModalTooltip opens, and verify ArrowRight/ArrowLeft still navigate.
		await grid.getByTestId('attachment-image-trigger').first().click();
		const lightbox = page.getByTestId('image-lightbox');
		await expect(lightbox).toBeVisible();
		await expect(lightbox).toContainText('first.png');
		await expect(lightbox).toContainText('1 of 2');

		await lightbox.getByRole('button', { name: 'Actual size', exact: true }).hover();
		await expect(lightbox.locator('[role="tooltip"]')).toBeVisible();

		await page.keyboard.press('ArrowRight');
		await expect(lightbox).toContainText('second.png');
		await expect(lightbox).toContainText('2 of 2');

		await page.keyboard.press('ArrowLeft');
		await expect(lightbox).toContainText('first.png');
		await expect(lightbox).toContainText('1 of 2');

		// Toolbar Next/Previous buttons also navigate cleanly.
		await lightbox.getByRole('button', { name: 'Next image', exact: true }).click();
		await expect(lightbox).toContainText('second.png');
		await lightbox.getByRole('button', { name: 'Previous image', exact: true }).click();
		await expect(lightbox).toContainText('first.png');

		// Escape dismisses an open tooltip first (WCAG 1.4.13), then closes the lightbox on second press.
		await lightbox.getByRole('button', { name: 'Actual size', exact: true }).hover();
		await expect(lightbox.locator('[role="tooltip"]')).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(lightbox.locator('[role="tooltip"]')).toHaveCount(0);
		await expect(lightbox).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(lightbox).toBeHidden();
		expect(pageErrors).toEqual([]);
	}
);
