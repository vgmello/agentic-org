import { createContext } from 'svelte';

export type NotificationTone = 'ok' | 'accent' | 'warn' | 'bad';
export type NotificationIcon = 'check' | 'pencil' | 'sparkle' | 'bell' | 'alert';

export interface BackgroundJob {
	id: string;
	title: string;
	step: string;
	source: 'tess' | 'system';
	startedAt: number;
}

export interface ShellNotification {
	id: string;
	title: string;
	summary: string;
	createdAt: number;
	href: string;
	icon: NotificationIcon;
	tone: NotificationTone;
	read: boolean;
}

export function formatRelativeNotificationTime(createdAt: number, now = Date.now()): string {
	const diffSec = Math.max(0, Math.floor((now - createdAt) / 1000));
	if (diffSec < 45) return 'Just now';
	const diffMin = Math.floor(diffSec / 60);
	if (diffMin < 60) return `${diffMin}m ago`;
	const diffHr = Math.floor(diffMin / 60);
	if (diffHr < 24) return `${diffHr}h ago`;
	return `${Math.floor(diffHr / 24)}d ago`;
}

/**
 * Reactive top-bar Notifications & Live Background Operations store (TASK-145).
 * Holds live in-progress operations (e.g. async Send to Tess task intakes) and
 * recent non-Inbox FYI completions. Never seeds fake static items.
 */
export class ShellNotificationsState {
	runningJobs = $state<BackgroundJob[]>([]);
	items = $state<ShellNotification[]>([]);
	open = $state(false);
	#seq = 0;

	get unreadCount(): number {
		let count = 0;
		for (const item of this.items) {
			if (!item.read) count += 1;
		}
		return count;
	}

	get activeTessJobs(): BackgroundJob[] {
		return this.runningJobs.filter((j) => j.source === 'tess');
	}

	toggle(): void {
		this.open = !this.open;
	}

	close(): void {
		this.open = false;
	}

	startJob(input: { id: string; title: string; step?: string; source?: 'tess' | 'system' }): void {
		const entry: BackgroundJob = {
			id: input.id,
			title: input.title,
			step: input.step ?? 'Processing in background…',
			source: input.source ?? 'tess',
			startedAt: Date.now()
		};
		const idx = this.runningJobs.findIndex((j) => j.id === entry.id);
		if (idx >= 0) {
			const next = [...this.runningJobs];
			next[idx] = entry;
			this.runningJobs = next;
		} else {
			this.runningJobs = [entry, ...this.runningJobs];
		}
	}

	finishJob(
		id: string,
		notice: {
			title: string;
			summary: string;
			href?: string;
			icon?: NotificationIcon;
			tone?: NotificationTone;
		}
	): void {
		this.runningJobs = this.runningJobs.filter((j) => j.id !== id);
		this.addNotice(notice);
	}

	failJob(id: string, title: string, summary: string, href = '/work'): void {
		this.runningJobs = this.runningJobs.filter((j) => j.id !== id);
		this.addNotice({ title, summary, href, icon: 'alert', tone: 'bad' });
	}

	addNotice(notice: {
		title: string;
		summary: string;
		href?: string;
		icon?: NotificationIcon;
		tone?: NotificationTone;
	}): void {
		const entry: ShellNotification = {
			id: `notif-${++this.#seq}-${Date.now()}`,
			title: notice.title,
			summary: notice.summary,
			createdAt: Date.now(),
			href: notice.href ?? '/work',
			icon: notice.icon ?? 'check',
			tone: notice.tone ?? 'ok',
			read: false
		};
		this.items = [entry, ...this.items].slice(0, 40);
	}

	markRead(id: string): void {
		this.items = this.items.map((item) => (item.id === id ? { ...item, read: true } : item));
		this.open = false;
	}

	markAllRead(): void {
		this.items = this.items.map((item) => (item.read ? item : { ...item, read: true }));
	}

	clearRead(): void {
		this.items = this.items.filter((item) => !item.read);
	}
}

const [getNotificationsContext, setNotificationsContext] = createContext<ShellNotificationsState>();

export function provideNotifications(
	store = new ShellNotificationsState()
): ShellNotificationsState {
	return setNotificationsContext(store);
}

export function useNotifications(): ShellNotificationsState | undefined {
	try {
		return getNotificationsContext();
	} catch {
		return undefined;
	}
}
