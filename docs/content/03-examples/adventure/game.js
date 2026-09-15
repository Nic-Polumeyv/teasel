import { roll } from './dice.js';
import { say } from './narrator.js';
import { createInterface } from 'node:readline/promises';
import { DIRECTIONS, LAMP_LIFE, look, restock } from './world.js';
import { Player } from './player.js';

let turns = 0;
let score = 0;
let lampTurns = LAMP_LIFE;

const verbs = {
	look: (player) => look(player, lampTurns),
	go: (player, direction) => act(player.go(direction)) || look(player, lampTurns),
	take: (player, item) => act(player.take(item)),
	drop: (player, item) => act(player.drop(item)),
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

function act(result) {
	if (result.ok) score += result.points;
	return result.text === '' ? null : say(result.text);
}

export function parse(line) {
	const words = line.trim().toLowerCase().split(/\s+/).filter(Boolean);
	if (words.length === 0) return null;
	let [verb, ...rest] = words;
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
	look(player, lampTurns);
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
	restock();
}
