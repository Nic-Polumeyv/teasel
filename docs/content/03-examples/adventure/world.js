import { describe, say } from './narrator.js';

export const DIRECTIONS = ['north', 'south', 'east', 'west', 'up', 'down'];
export const LAMP_LIFE = 40;

export const rooms = {
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

export function look(player, lampTurns) {
	const room = rooms[player.here];
	if (room.dark && !(player.has('lamp') && lampTurns > 0)) return say('It is pitch black. You are likely to be eaten by a grue.');
	const lines = [describe(room.text)];
	if (room.items.length > 0) lines.push(`You see: ${room.items.join(', ')}.`);
	lines.push(`Exits: ${Object.keys(room.exits).join(', ')}.`);
	return say(lines.join('\n'));
}

export function restock() {
	rooms.cellar.items = ['lamp'];
	rooms.kitchen.items = ['key', 'bread'];
	rooms.hall.items = [];
	rooms.tower.items = ['teasel'];
}
