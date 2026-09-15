import { rooms } from './world.js';

const MAX_CARRY = 5;

export class Player {
	#room = 'cellar';
	#carrying = [];
	#seen = new Set(['cellar']);

	get here() {
		return this.#room;
	}

	get room() {
		return rooms[this.#room];
	}

	get inventory() {
		return [...this.#carrying];
	}

	has(item) {
		return this.#carrying.includes(item);
	}

	take(item) {
		const { items } = this.room;
		if (!items.includes(item)) return { ok: false, text: `There is no ${item} here.` };
		if (this.#carrying.length >= MAX_CARRY) return { ok: false, text: 'Your hands are full.' };
		items.splice(items.indexOf(item), 1);
		this.#carrying.push(item);
		return { ok: true, text: `Taken: ${item}.`, points: 5 };
	}

	drop(item) {
		if (!this.has(item)) return { ok: false, text: `You have no ${item}.` };
		this.#carrying.splice(this.#carrying.indexOf(item), 1);
		this.room.items.push(item);
		return { ok: true, text: `Dropped: ${item}.`, points: 0 };
	}

	go(direction) {
		const { exits, locked = {} } = this.room;
		const next = exits[direction];
		if (!next) return { ok: false, text: 'You cannot go that way.' };
		if (locked[direction] && !this.has(locked[direction])) return { ok: false, text: `The way ${direction} is locked.` };
		this.#room = next;
		const first = !this.#seen.has(next);
		this.#seen.add(next);
		return { ok: true, text: '', points: first ? 10 : 0 };
	}
}
