import { expect, test, type Page } from '@playwright/test';
import { enterMockedOrganization } from './mock-organization';
import { visit } from './ready';

// Valid 1x1 red PNG (67 bytes, valid IHDR 1x1 so both browser decode & PNG dimension sniffer succeed).
const PNG_1X1_BASE64 =
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC';

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

		// 1. Attach PNG image -> staged preview renders immediately with blob: URL + vision note.
		await page.getByLabel('Attach files').setInputFiles({
			name: 'diagram.png',
			mimeType: 'image/png',
			buffer: Buffer.from(PNG_1X1_BASE64, 'base64')
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
		const imageTrigger = page.getByTestId('attachment-image-trigger').last();
		await expect(imageTrigger).toBeVisible();
		const renderedImg = imageTrigger.locator('img');
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
