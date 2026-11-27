import { tick } from 'svelte';
import { RowActivity, type ChatMessage, type CommandForm } from '$lib/modules/messaging';
import { isTextInputTarget } from '../../helpers';
import type { ChatPanelKind } from '../../types';
import { focusVisible as focusChatTarget, revealFocus } from './chat-focus';

export interface ChatOverlayDeps {
	getCompactPanels: () => boolean;
	getPanelKind: () => ChatPanelKind | undefined;
	getOfficeRunning: () => boolean;
	getConversationEl: () => HTMLElement | undefined;
	getListEl: () => HTMLElement | undefined;
	getHeading: () => HTMLHeadingElement | undefined;
	getSidebarToggle: () => HTMLButtonElement | HTMLAnchorElement | null;
	getPanel: () => { focusClose: () => void } | undefined;
	getDialogs: () => { dismissOpenDialogs: () => void } | undefined;
	getNavigation: () => { focusSelected: (preferred?: HTMLElement) => void } | undefined;
	onWritePlace: () => void;
	onSelectConversation: (id: string, navigate?: boolean) => void;
	onResetSearchForThread: (conversationId: string) => void;
}

/**
 * Manages ChatWorkspace overlay, side-panel, and focus-restoration state (zero `$effect`).
 */
export class ChatOverlayController {
	readonly rowActivity = new RowActivity();
	threadRoot = $state<string | undefined>();
	showMembers = $state(false);
	showPins = $state(false);
	highlight = $state<string | undefined>();
	sidebarInline = $state(true);
	sidebarOpen = $state(false);
	sidebarDialogMode = $state(false);
	command = $state<CommandForm | undefined>();
	commandOpen = $state(false);
	commandOpener = $state<HTMLElement | undefined>();
	channelMode = $state<'create' | 'rename' | 'members'>('create');
	channelOpen = $state(false);
	dmOpen = $state(false);
	browseOpen = $state(false);

	private threadOpener: HTMLElement | undefined;
	private membersOpener: HTMLElement | undefined;
	private pinsOpener: HTMLElement | undefined;
	private browseOpener: HTMLElement | undefined;
	private sidebarReturn: 'toggle' | 'selection' | 'dialog' = 'toggle';
	private sidebarDialogSelected = false;
	private sidebarDialogOpener: HTMLElement | undefined;
	private focusRaf: number | undefined;

	constructor(private readonly deps: ChatOverlayDeps) {}

	dispose = () => {
		if (this.focusRaf !== undefined) {
			cancelAnimationFrame(this.focusRaf);
			this.focusRaf = undefined;
		}
	};

	rowIsActive = (message: ChatMessage) => this.rowActivity.active(message.id);

	focusVisible = (target: HTMLElement | null | undefined) =>
		focusChatTarget(target, this.deps.getCompactPanels());

	menuChanged = (message: ChatMessage, open: boolean, opener: HTMLElement) => {
		if (open) this.rowActivity.hold(message.id, opener, 'menu');
		else {
			this.rowActivity.closeMenu(opener);
			void tick().then(() => this.rowActivity.focused(document.activeElement));
		}
	};

	revealChatFocus = (event: FocusEvent) => {
		const target = event.target;
		this.rowActivity.focused(target);
		if (
			!(target instanceof HTMLElement) ||
			!(this.deps.getConversationEl()?.contains(target) || target.closest('[data-chat-panel]'))
		)
			return;
		if (this.focusRaf !== undefined) cancelAnimationFrame(this.focusRaf);
		this.focusRaf = requestAnimationFrame(() => {
			this.focusRaf = undefined;
			if (target.isConnected && document.activeElement === target) revealFocus(target);
		});
	};

	hasModalOverlay = () =>
		[this.sidebarOpen, this.channelOpen, this.dmOpen, this.commandOpen, this.browseOpen].some(
			Boolean
		);

	hasOpenOverlay = () => this.hasModalOverlay() || this.showMembers || this.showPins;

	dismissAllOverlays = () => {
		if (this.commandOpen || this.browseOpen || this.channelOpen || this.dmOpen) {
			this.deps.getDialogs()?.dismissOpenDialogs();
			return;
		}
		if (this.sidebarOpen) {
			this.sidebarReturn = 'toggle';
			this.sidebarOpen = false;
			return;
		}
		if (this.showMembers) {
			this.closeMembers();
			return;
		}
		if (this.showPins) {
			this.closePins();
		}
	};

	openSidebar = () => {
		this.sidebarReturn = 'toggle';
		this.sidebarOpen = true;
	};

	resetForViewChange = () => {
		if (this.sidebarOpen) {
			this.sidebarReturn = 'selection';
			this.sidebarOpen = false;
		}
		if (this.sidebarDialogMode) this.sidebarDialogSelected = true;
		this.threadRoot = undefined;
		this.threadOpener = undefined;
		this.rowActivity.clear();
		this.showMembers = false;
		this.membersOpener = undefined;
		this.showPins = false;
		this.pinsOpener = undefined;
		this.highlight = undefined;
	};

	focusAfterNavigation = (selected: boolean) => {
		if (this.deps.getCompactPanels() && this.deps.getPanelKind())
			this.deps.getPanel()?.focusClose();
		else if (selected) this.focusVisible(this.deps.getHeading());
		else if (this.sidebarInline) this.deps.getNavigation()?.focusSelected();
		else this.focusVisible(this.deps.getSidebarToggle());
	};

	closeSidebarFocus = () => {
		const reason = this.sidebarReturn;
		this.sidebarReturn = 'toggle';
		if (reason !== 'dialog')
			void tick().then(() => this.focusAfterNavigation(reason === 'selection'));
	};

	openChannelDialog = (mode: 'create' | 'rename' | 'members') => {
		if (!this.deps.getOfficeRunning()) return;
		this.channelMode = mode;
		this.channelOpen = true;
	};

	startSidebarDialog = (kind: 'channel' | 'dm', opener: HTMLElement) => {
		if (!this.deps.getOfficeRunning()) return;
		this.sidebarDialogMode = true;
		this.sidebarDialogSelected = false;
		this.sidebarDialogOpener = opener;
		if (this.sidebarOpen) {
			this.sidebarReturn = 'dialog';
			this.sidebarOpen = false;
		}
		if (kind === 'channel') this.openChannelDialog('create');
		else this.dmOpen = true;
	};

	finishSidebarDialog = () => {
		const selected = this.sidebarDialogSelected;
		const opener = this.sidebarDialogOpener;
		this.sidebarDialogMode = false;
		this.sidebarDialogOpener = undefined;
		void tick().then(() => {
			if (this.sidebarInline) this.deps.getNavigation()?.focusSelected(opener);
			else this.focusAfterNavigation(selected);
		});
	};

	closeThread = (restore = true) => {
		const wasOpen = !!this.threadRoot;
		const opener = this.threadOpener;
		this.rowActivity.returning(opener, 'thread');
		this.threadRoot = undefined;
		this.highlight = undefined;
		this.deps.onWritePlace();
		const target = this.threadOpener?.isConnected ? this.threadOpener : this.deps.getListEl();
		if (wasOpen && restore)
			void tick().then(() => {
				if (this.threadRoot || this.threadOpener !== opener)
					return this.rowActivity.focused(opener);
				this.focusVisible(target);
				this.rowActivity.focused(opener);
				this.threadOpener = undefined;
			});
		else {
			this.rowActivity.release(opener, 'thread');
			this.threadOpener = undefined;
		}
	};

	closeMembers = (restore = true) => {
		this.showMembers = false;
		if (restore)
			void tick().then(() =>
				this.focusVisible(
					this.membersOpener?.isConnected ? this.membersOpener : this.deps.getListEl()
				)
			);
	};

	closePins = (restore = true) => {
		this.showPins = false;
		if (restore)
			void tick().then(() =>
				this.focusVisible(this.pinsOpener?.isConnected ? this.pinsOpener : this.deps.getListEl())
			);
	};

	toggleMembers = (opener: HTMLElement) => {
		if (this.showMembers) return this.closeMembers();
		this.membersOpener = opener;
		this.showMembers = true;
		this.showPins = false;
		this.closeThread(false);
	};

	togglePins = (opener: HTMLElement) => {
		if (this.showPins) return this.closePins();
		this.pinsOpener = opener;
		this.showPins = true;
		this.showMembers = false;
		this.closeThread(false);
	};

	openThread = (m: ChatMessage, opener: HTMLElement) => {
		this.deps.onResetSearchForThread(m.conversation_id);
		this.rowActivity.release(this.threadOpener, 'thread');
		this.threadOpener = opener;
		this.rowActivity.hold(m.id, opener, 'thread');
		this.threadRoot = m.thread_root_id ?? m.id;
		this.showMembers = false;
		this.showPins = false;
		this.deps.onWritePlace();
	};

	returnCommandFocus = () => {
		const opener = this.commandOpener;
		this.rowActivity.returning(opener, 'dialog');
		void tick().then(() => {
			if (this.commandOpen || this.commandOpener !== opener)
				return this.rowActivity.focused(opener);
			this.focusVisible(opener?.isConnected ? opener : this.deps.getListEl());
			this.rowActivity.focused(opener);
			this.commandOpener = undefined;
		});
	};

	openBrowse = (opener: HTMLElement) => {
		this.browseOpener = opener;
		if (this.sidebarOpen) {
			this.sidebarReturn = 'dialog';
			this.sidebarOpen = false;
		}
		this.browseOpen = true;
	};

	browseReturn = () => {
		const opener = this.browseOpener;
		this.browseOpener = undefined;
		void tick().then(() => {
			if (document.activeElement !== document.body) return;
			if (opener?.isConnected) opener.focus();
			else if (this.sidebarInline) this.deps.getNavigation()?.focusSelected();
			else this.focusVisible(this.deps.getSidebarToggle());
		});
	};

	browseShow = (id: string) => {
		this.browseOpener = undefined;
		this.browseOpen = false;
		this.deps.onSelectConversation(id, true);
		void tick().then(() => this.focusVisible(this.deps.getHeading()));
	};

	handleWindowKeydown = (event: KeyboardEvent) => {
		const kind = this.deps.getPanelKind();
		if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing) return;
		if (!kind || this.hasModalOverlay() || isTextInputTarget(event.target)) return;
		event.preventDefault();
		if (kind === 'thread') this.closeThread();
		else if (kind === 'members') this.closeMembers();
		else this.closePins();
	};
}
