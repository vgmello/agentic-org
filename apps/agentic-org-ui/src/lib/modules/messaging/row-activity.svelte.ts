type Owner = 'menu' | 'thread' | 'dialog';

/** Chat owns each opener's lifetime; DOM focus stays with the route and its panels. */
export class RowActivity {
	#entries = $state.raw(new Map<EventTarget, { id: string; owners: ReadonlySet<Owner> }>());

	active(id: string) {
		return [...this.#entries.values()].some(entry => entry.id === id);
	}
	hold(id: string, opener: EventTarget, owner: Owner) {
		const owners = new Set(this.#entries.get(opener)?.owners).add(owner);
		this.#entries = new Map(this.#entries).set(opener, { id, owners });
	}
	closeMenu(opener: EventTarget) { this.returning(opener, 'menu'); }
	returning(opener: EventTarget | undefined, owner: Owner) {
		const entry = opener && this.#entries.get(opener);
		if (entry) {
			const owners = new Set(entry.owners);
			owners.delete(owner);
			this.#entries = new Map(this.#entries).set(opener!, { id: entry.id, owners });
		}
	}
	focused(opener: EventTarget | null | undefined) {
		if (opener && this.#entries.get(opener)?.owners.size === 0) {
			const next = new Map(this.#entries);
			next.delete(opener);
			this.#entries = next;
		}
	}
	release(opener: EventTarget | undefined, owner: Owner) {
		this.returning(opener, owner);
		this.focused(opener);
	}
	clear() { this.#entries = new Map(); }
}
