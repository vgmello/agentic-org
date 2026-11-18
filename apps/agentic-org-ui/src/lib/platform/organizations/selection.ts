/**
 * Which organization this browser has open, per instance and account. It is a preference,
 * not an authorization: the server decides what an account can open on every request.
 * Only IDs are stored; a damaged or foreign value reads as no selection.
 */
export interface SelectionOwner {
	instanceId: string;
	accountId: string;
}
type KeyValue = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;
const KEY = 'org-selection';
const ID = /^[A-Za-z0-9-]{1,64}$/;

export function createSelection(storage: () => KeyValue | undefined) {
	const read = (): Record<string, string> => {
		try {
			const value: unknown = JSON.parse(storage()?.getItem(KEY) ?? '{}');
			if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
			return Object.fromEntries(
				Object.entries(value).filter(
					(entry): entry is [string, string] => typeof entry[1] === 'string' && ID.test(entry[1])
				)
			);
		} catch {
			return {};
		}
	};
	const write = (next: Record<string, string>) => {
		try {
			if (Object.keys(next).length === 0) storage()?.removeItem(KEY);
			else storage()?.setItem(KEY, JSON.stringify(next));
		} catch {
			// Storage is unavailable (private mode, quota): the choice lasts for this page only.
		}
	};
	const key = ({ instanceId, accountId }: SelectionOwner) => `${instanceId}/${accountId}`;
	return Object.freeze({
		current: (owner: SelectionOwner): string | undefined => {
			const all = read();
			return Object.hasOwn(all, key(owner)) ? all[key(owner)] : undefined;
		},
		select(owner: SelectionOwner, organizationId: string): void {
			if (!ID.test(organizationId)) throw new Error('Invalid organization ID');
			write({ ...read(), [key(owner)]: organizationId });
		},
		clear(owner: SelectionOwner): void {
			const { [key(owner)]: _removed, ...rest } = read();
			write(rest);
		}
	});
}

/** The browser's selection store. */
export const selection = createSelection(() =>
	typeof localStorage === 'undefined' ? undefined : localStorage
);
