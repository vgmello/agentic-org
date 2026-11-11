import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { selectDesktopSection } from './desktop-navigation';
import { focusDestinationHeading } from './focus-destination';

const click = (overrides: Partial<MouseEvent> = {}) => ({
	defaultPrevented: false, button: 0, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false,
	preventDefault: vi.fn(), ...overrides
}) as unknown as MouseEvent;

let active: unknown;
let link: object;
let heading: { tabIndex: number; focus: ReturnType<typeof vi.fn> };
let query: ReturnType<typeof vi.fn>;

beforeEach(() => {
	link = { name: 'clicked link' };
	active = link;
	heading = { tabIndex: 0, focus: vi.fn(() => { active = heading; }) };
	query = vi.fn((selector: string) => { expect(selector).toBe('main h1'); return heading; });
	vi.stubGlobal('window', { location: { href: 'http://localhost/offices' } });
	vi.stubGlobal('document', { querySelector: query, get activeElement() { return active; } });
});
afterEach(() => vi.unstubAllGlobals());

describe('desktop Sidebar section selection', () => {
	it.each([
		['defaultPrevented', { defaultPrevented: true }], ['middle button', { button: 1 }],
		['right button', { button: 2 }], ['meta', { metaKey: true }], ['ctrl', { ctrlKey: true }],
		['shift', { shiftKey: true }], ['alt', { altKey: true }]
	] as const)('leaves %s clicks to their existing handler/browser', async (_name, overrides) => {
		const event = click(overrides), navigate = vi.fn();
		await selectDesktopSection(event, '/agents', navigate);
		expect(event.preventDefault).not.toHaveBeenCalled();
		expect(navigate).not.toHaveBeenCalled();
		expect(query).not.toHaveBeenCalled();
		expect(document.activeElement).toBe(link);
	});

	it('leaves Switch organization and non-shell links untouched', async () => {
		for (const href of ['/welcome', '/create', 'https://example.test/agents']) {
			const event = click(), navigate = vi.fn();
			await selectDesktopSection(event, href, navigate);
			expect(event.preventDefault).not.toHaveBeenCalled();
			expect(navigate).not.toHaveBeenCalled();
		}
		expect(query).not.toHaveBeenCalled();
	});

	it('prevents a plain click and focuses only after navigation resolves', async () => {
		let release!: () => void;
		const navigate = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
		const event = click();
		const selected = selectDesktopSection(event, '/agents', navigate);
		expect(event.preventDefault).toHaveBeenCalledTimes(1);
		expect(navigate).toHaveBeenCalledExactlyOnceWith('/agents');
		expect(query).not.toHaveBeenCalled();
		expect(document.activeElement).toBe(link);
		release();
		await selected;
		expect(document.activeElement).toBe(heading);
		expect(heading.tabIndex).toBe(-1);
		expect(heading.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
	});

	it('reselects the same URL by focusing without a navigation/history entry', async () => {
		const event = click(), navigate = vi.fn();
		await selectDesktopSection(event, '/offices', navigate);
		expect(event.preventDefault).toHaveBeenCalledTimes(1);
		expect(navigate).not.toHaveBeenCalled();
		expect(document.activeElement).toBe(heading);
	});

	it.each(['?sort=name', '#notes'])('compares the full current URL, including %s', async (suffix) => {
		window.location.href = `http://localhost/offices${suffix}`;
		const navigate = vi.fn().mockResolvedValue(undefined);
		await selectDesktopSection(click(), '/offices', navigate);
		expect(navigate).toHaveBeenCalledExactlyOnceWith('/offices');
		expect(document.activeElement).toBe(heading);
	});

	it('a rejected navigation leaves the clicked link focused and never queries the title', async () => {
		const navigate = vi.fn().mockRejectedValue(new Error('SvelteKit navigation cancelled'));
		await expect(selectDesktopSection(click(), '/agents', navigate)).resolves.toBeUndefined();
		expect(navigate).toHaveBeenCalledExactlyOnceWith('/agents');
		expect(query).not.toHaveBeenCalled();
		expect(heading.focus).not.toHaveBeenCalled();
		expect(document.activeElement).toBe(link);
	});
});

describe('shared mobile/desktop destination heading focus', () => {
	it('waits for the Svelte tick before querying the new heading', async () => {
		const focused = focusDestinationHeading();
		expect(query).not.toHaveBeenCalled();
		await focused;
		expect(query).toHaveBeenCalledExactlyOnceWith('main h1');
		expect(heading.tabIndex).toBe(-1);
		expect(heading.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
	});

	it('does nothing when the destination has no main heading', async () => {
		query.mockReturnValue(null);
		await expect(focusDestinationHeading()).resolves.toBeUndefined();
		expect(heading.focus).not.toHaveBeenCalled();
		expect(document.activeElement).toBe(link);
	});
});
