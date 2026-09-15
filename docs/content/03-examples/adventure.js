import { roll } from './dice.js';
import { describe, listen, say } from './narrator.js';
import { createInterface } from 'node:readline/promises';

const DIRECTIONS = ['north', 'south', 'east', 'west', 'up', 'down'];
const MAX_CARRY = 5;
const LAMP_LIFE = 40;

let turns = 0;
let score = 0;
let lampTurns = LAMP_LIFE;

const rooms = {
	cellar: {
		dark: true,
		text: 'A damp cellar. Stairs climb into gloom.',
		exits: { up: 'kitchen' },
		items: ['lamp'],
	},
	kitchen: {
		text: 'A kitchen last used a century ago. A door leads east, a stair down.',
		exits: { down: 'cellar', east: 'hall' },
		items: ['key', 'bread'],
	},
	hall: {
		text: 'A long hall of portraits. Their eyes follow you north.',
		exits: { west: 'kitchen', north: 'tower' },
		locked: { north: 'key' },
		items: [],
	},
	tower: {
		text: 'The tower room. Wind, sky, and a very old teasel in a pot.',
		exits: { south: 'hall' },
		items: ['teasel'],
	},
};

export class Player {
	#room = 'cellar';
	#carrying = [];
	#seen = new Set();

	get room() {
		return rooms[this.#room];
	}

	get here() {
		return this.#room;
	}

	get inventory() {
		return [...this.#carrying];
	}

	has(item) {
		return this.#carrying.includes(item);
	}

	take(item) {
		const { items } = this.room;
		if (!items.includes(item)) return say(`There is no ${item} here.`);
		if (this.#carrying.length >= MAX_CARRY) return say('Your hands are full.');
		items.splice(items.indexOf(item), 1);
		this.#carrying.push(item);
		score += 5;
		return say(`Taken: ${item}.`);
	}

	drop(item) {
		if (!this.has(item)) return say(`You have no ${item}.`);
		this.#carrying.splice(this.#carrying.indexOf(item), 1);
		this.room.items.push(item);
		return say(`Dropped: ${item}.`);
	}

	go(direction) {
		const { exits, locked = {} } = this.room;
		const next = exits[direction];
		if (!next) return say('You cannot go that way.');
		if (locked[direction] && !this.has(locked[direction])) return say(`The way ${direction} is locked.`);
		this.#room = next;
		if (!this.#seen.has(next)) {
			this.#seen.add(next);
			score += 10;
		}
		return look(this);
	}
}

export function look(player) {
	const room = player.room;
	if (room.dark && !(player.has('lamp') && lampTurns > 0)) return say('It is pitch black. You are likely to be eaten by a grue.');
	const lines = [describe(room.text)];
	if (room.items.length > 0) lines.push(`You see: ${room.items.join(', ')}.`);
	const ways = Object.keys(room.exits).join(', ');
	lines.push(`Exits: ${ways}.`);
	return say(lines.join('\n'));
}

const verbs = {
	look: (player) => look(player),
	go: (player, direction) => player.go(direction),
	take: (player, item) => player.take(item),
	drop: (player, item) => player.drop(item),
	inventory: (player) => say(player.inventory.length ? `You carry: ${player.inventory.join(', ')}.` : 'You carry nothing.'),
	eat: (player, item) => {
		if (item !== 'bread' || !player.has('bread')) return say('You cannot eat that.');
		player.drop('bread');
		player.room.items.pop();
		score += roll(6);
		return say('Stale, but filling.');
	},
	score: () => say(`Score: ${score} in ${turns} turns.`),
};

export function parse(line) {
	const words = line.trim().toLowerCase().split(/\s+/).filter(Boolean);
	if (words.length === 0) return null;
	let [verb, ...rest] = words;
	if (DIRECTIONS.includes(verb)) return { verb: 'go', object: verb };
	if (verb === 'n' || verb === 's' || verb === 'e' || verb === 'w') verb = { n: 'north', s: 'south', e: 'east', w: 'west' }[verb];
	if (DIRECTIONS.includes(verb)) return { verb: 'go', object: verb };
	if (verb === 'i' || verb === 'inv') verb = 'inventory';
	if (verb === 'l') verb = 'look';
	if (verb === 'get' || verb === 'grab') verb = 'take';
	const object = rest.filter((w) => w !== 'the' && w !== 'a').join(' ');
	return { verb, object };
}

export function step(player, line) {
	const command = parse(line);
	if (command === null) return say('Say something.');
	const run = verbs[command.verb];
	if (run === undefined) return say(`I do not know how to ${command.verb}.`);
	turns += 1;
	if (player.has('lamp') && lampTurns > 0) {
		lampTurns -= 1;
		if (lampTurns === 0) say('Your lamp flickers out.');
	}
	return run(player, command.object);
}

export function won(player) {
	return player.here === 'tower' && player.has('teasel');
}

export async function play(input = process.stdin, output = process.stdout) {
	const player = new Player();
	const prompt = createInterface({ input, output });
	look(player);
	while (!won(player)) {
		const line = await prompt.question('> ');
		if (line === 'quit') break;
		step(player, line);
	}
	prompt.close();
	if (won(player)) say(`You carry the teasel into the light. ${score} points in ${turns} turns.`);
	return { score, turns };
}

export function reset() {
	turns = 0;
	score = 0;
	lampTurns = LAMP_LIFE;
	rooms.cellar.items = ['lamp'];
	rooms.kitchen.items = ['key', 'bread'];
	rooms.hall.items = [];
	rooms.tower.items = ['teasel'];
}

export const hint = listen((player) => {
	if (player.here === 'cellar' && !player.has('lamp')) return 'Feel around. Something is on the floor.';
	if (player.here === 'hall' && !player.has('key')) return 'The kitchen had more than bread in it.';
	return null;
});
