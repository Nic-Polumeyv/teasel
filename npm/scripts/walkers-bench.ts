// node scripts/walkers-bench.ts [--rounds N] [--warmup N] [--scopes] DIR...: every .svelte file under the directories
// parsed by the Svelte grammar's walker and by the walker written for Svelte, whole and with script and style cut out,
// and the times compared. Each walker and corpus is timed in a process of its own, `--rounds` kept rounds after
// `--warmup` ones, with no forced collection between them; the time inside `engine.parse` is taken apart from the
// decoder's. `npm run build:js` and the addon or `npm run build:wasm` first; `node --no-addons` times WebAssembly.
// Without a directory it reads Svelte's own tests in crates/teasel/tests/svelte.
import { fork } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import type { Options } from '../src/options.ts';

type Walker = 'grammar' | 'svelte';
interface Request {
	walker: Walker;
	inputs: string[];
	rounds: number;
	warmup: number;
	options: Options;
}
/** Per kept round: the whole corpus's time, and the part spent in `engine.parse`. */
interface Answer {
	totals: number[];
	engines: number[];
}

// a child: one walker over one corpus, alone, because the decoder builds code per option set and language
if (process.argv[2] === '--child') {
	process.once('message', async ({ walker, inputs, rounds, warmup, options }: Request) => {
		// index.ts reaches the engine through #engine, which is this module, so the wrapper sees every parse
		const { engine } = await import('../dist/engine/node.js');
		const { parse } = await import('../src/index.ts');
		const language = walker === 'grammar' ? (await import('./hosts/svelte.ts')).default : (await import('../src/svelte.ts')).svelte;
		let inside = 0;
		const own = engine.parse;
		(engine as { parse: typeof own }).parse = (...args) => {
			const start = performance.now();
			try {
				return Reflect.apply(own, engine, args);
			} finally {
				inside += performance.now() - start;
			}
		};
		const answer: Answer = { totals: [], engines: [] };
		for (let round = 0; round < warmup + rounds; round++) {
			inside = 0;
			const start = performance.now();
			for (const text of inputs) parse(text, options, language as never);
			const total = performance.now() - start;
			if (round < warmup) continue;
			answer.totals.push(total);
			answer.engines.push(inside);
		}
		process.send!(answer, () => process.exit(0));
	});
} else {
	await main();
}

async function main() {
	const args = process.argv.slice(2);
	let rounds = 40;
	let warmup = 10;
	let scopes = false;
	const dirs: string[] = [];
	for (let i = 0; i < args.length; i++) {
		const arg = args[i];
		if (arg === '--scopes') scopes = true;
		else if (arg === '--rounds' || arg === '--warmup') {
			const n = Number(args[++i]);
			if (!Number.isInteger(n) || n < (arg === '--rounds' ? 1 : 0)) throw new Error(`${arg} takes a count`);
			if (arg === '--rounds') rounds = n;
			else warmup = n;
		} else if (arg.startsWith('--')) throw new Error(`${arg} is not an option: --rounds N, --warmup N, --scopes`);
		else dirs.push(arg);
	}
	if (dirs.length === 0) {
		const tests = new URL('../../crates/teasel/tests/svelte/packages/svelte/tests/', import.meta.url).pathname;
		if (!existsSync(tests)) throw new Error('pass directories of .svelte files, or clone Svelte\'s tests as CI does');
		dirs.push(tests);
	}
	const options: Options = { sourceType: 'module', comments: true, scopes };

	// the same component copied across packages would weigh as many times as it appears
	const paths: string[] = [];
	const walk = (dir: string) => {
		for (const name of readdirSync(dir)) {
			if (name === 'node_modules' || name.startsWith('.')) continue;
			const path = join(dir, name);
			if (statSync(path).isDirectory()) walk(path);
			else if (name.endsWith('.svelte')) paths.push(path);
		}
	};
	for (const dir of dirs) walk(dir);
	const unique = [...new Set(paths.map((path) => readFileSync(path, 'utf8')))];

	// only what parses is timed, and the tree tells where the scripts and the style sit
	const { parse } = await import('../src/index.ts');
	const { svelte } = await import('../src/svelte.ts');
	const docs: string[] = [];
	const templates: string[] = [];
	let rejected = 0;
	for (const text of unique) {
		let root: { instance?: { start: number; end: number } | null; module?: { start: number; end: number } | null; css: { start: number; end: number } | null };
		try {
			root = parse(text, options, svelte).node as never;
		} catch {
			rejected++;
			continue;
		}
		docs.push(text);
		let template = text;
		const cuts = [root.instance, root.module, root.css].filter((cut) => cut != null).sort((a, b) => b.start - a.start);
		for (const { start, end } of cuts) template = template.slice(0, start) + template.slice(end);
		try {
			parse(template, options, svelte);
			templates.push(template);
		} catch {
			// a template whose TypeScript lost its script
		}
	}

	const time = (walker: Walker, inputs: string[]) =>
		new Promise<Answer>((resolve, reject) => {
			const child = fork(new URL(import.meta.url).pathname, ['--child'], { execArgv: [...process.execArgv, '--no-warnings'] });
			child.once('message', (answer: Answer) => resolve(answer));
			child.once('exit', (code) => (code === 0 ? undefined : reject(new Error(`${walker} exited with ${code}`))));
			child.send({ walker, inputs, rounds, warmup, options } satisfies Request);
		});
	const stats = (values: number[]) => {
		const sorted = [...values].sort((a, b) => a - b);
		return { p10: sorted[Math.floor((sorted.length - 1) * 0.1)], median: sorted[Math.floor((sorted.length - 1) / 2)] };
	};
	const bytes = (inputs: string[]) => inputs.reduce((n, text) => n + Buffer.byteLength(text), 0);
	const ms = (n: number) => n.toFixed(2).padStart(9);
	const { load } = await import('../dist/engine/addon.js');
	const emitWarning = process.emitWarning;
	process.emitWarning = () => {};
	const native = load() !== undefined;
	process.emitWarning = emitWarning;
	console.log(`${unique.length} unique .svelte files, ${rejected} rejected; ${native ? 'native addon' : 'WebAssembly'}, Node ${process.version}; ${rounds} rounds after ${warmup} warm-up, each row in its own process${scopes ? '; with scopes' : ''}`);
	console.log(`\n${''.padEnd(34)}${'p10 ms'.padStart(9)}${'median'.padStart(11)}${'engine p10'.padStart(12)}${'engine med'.padStart(12)}${'decode med'.padStart(12)}${'MB/s'.padStart(8)}`);
	for (const [label, inputs] of [
		['whole documents', docs],
		['without script or style', templates],
	] as const) {
		console.log(`${label}: ${inputs.length} files, ${(bytes(inputs) / 1024).toFixed(1)} kB`);
		const medians: number[] = [];
		for (const walker of ['grammar', 'svelte'] as const) {
			const answer = await time(walker, inputs);
			const total = stats(answer.totals), engine = stats(answer.engines);
			const decode = stats(answer.totals.map((t, i) => t - answer.engines[i]));
			medians.push(engine.median);
			const speed = bytes(inputs) / 1e6 / (engine.median / 1e3);
			console.log(`  ${walker === 'grammar' ? 'grammar walker' : 'svelte walker'}`.padEnd(34) + `${ms(total.p10)}  ${ms(total.median)}   ${ms(engine.p10)}   ${ms(engine.median)}   ${ms(decode.median)}${speed.toFixed(1).padStart(8)}`);
		}
		console.log(`  engine time, svelte walker over grammar walker: ${(medians[1] / medians[0]).toFixed(3)}`);
	}
}
