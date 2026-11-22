// node scripts/bench.ts [--rounds N] [--warmup N] [--gc] [--json] DIR...: every .svelte file under the directories, parsed by
// Svelte's parser and by teasel, and the times compared. `npm run build:js` first, and the addon
// or `npm run build:wasm`; `node --no-addons scripts/bench.ts` times WebAssembly. Without a directory it reads the
// Svelte host tests, which are too few and too small to stand for real components.
import { fork } from 'node:child_process';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

// the trees are read as Svelte shapes them, past what its types say
type Any = any;
/** A source and whether its scripts are TypeScript. */
type Input = [text: string, typescript: boolean];
type Job = 'svelte' | 'public' | 'teasel' | 'scopes' | 'scripts' | 'templates';
interface Request {
	job: Job;
	inputs: Input[];
	rounds: number;
	warmup: number;
	gc: boolean;
}
/** Per kept round: the whole corpus's time, and the part spent in `engine.parse`. */
interface Answer {
	totals: number[];
	engines: number[];
}

// Svelte's public `parse` walks the tree once more to delete `metadata` from every node, which `compile` never pays,
// so the parser `compile` calls is the baseline; the package's exports do not reach it, so it comes in by path
const svelteDir = new URL('../node_modules/svelte/', import.meta.url);
async function svelteInternal(): Promise<(text: string) => unknown> {
	const { parse }: Any = await import(new URL('src/compiler/phases/1-parse/index.js', svelteDir).href);
	const state: Any = await import(new URL('src/compiler/state.js', svelteDir).href);
	return (text) => {
		state.reset({ warning: () => false, filename: undefined });
		return parse(text);
	};
}

// a child: one job, timed alone, because the decoder builds code per option set, and one process given several parses each of them slower
if (process.argv[2] === '--child') {
	process.once('message', async ({ job, inputs, rounds, warmup, gc }: Request) => {
		let inside = 0;
		let run: (input: Input) => unknown;
		if (job === 'svelte') {
			const parse = await svelteInternal();
			run = ([text]) => parse(text);
		} else if (job === 'public') {
			const { parse } = await import('svelte/compiler');
			run = ([text]) => parse(text, { modern: true });
		} else {
			// index.ts reaches the engine through #engine, which is this module, so the wrapper sees every parse
			const { engine } = await import('../dist/engine/node.js');
			const { parse, parseModule } = await import('../src/index.ts');
			const own = engine.parse;
			(engine as { parse: typeof own }).parse = (...args) => {
				const start = performance.now();
				try {
					return Reflect.apply(own, engine, args);
				} finally {
					inside += performance.now() - start;
				}
			};
			run =
				job === 'teasel' ? ([text]) => parse(text, { comments: true })
				: job === 'scopes' ? ([text]) => parse(text, { comments: true, scopes: true })
				: job === 'scripts' ? ([text, typescript]) => parseModule(text, { comments: true, typescript })
				: ([text, typescript]) => parse(text, { comments: true, typescript });
		}
		const answer: Answer = { totals: [], engines: [] };
		for (let round = 0; round < warmup + rounds; round++) {
			// a full collection before each round slowed the next round's decoding about twice over, engine time barely moving: decode share 21–22% without and 36–38% with on WebAssembly, 27% and 42% on the addon (2026-11-21)
			if (gc) globalThis.gc!();
			inside = 0;
			const start = performance.now();
			for (const input of inputs) run(input);
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
	let json = false;
	let gc = false;
	const dirs: string[] = [];
	for (let i = 0; i < args.length; i++) {
		const arg = args[i];
		if (arg === '--json') json = true;
		else if (arg === '--gc') gc = true;
		else if (arg === '--rounds' || arg === '--warmup') {
			const n = Number(args[++i]);
			if (!Number.isInteger(n) || n < (arg === '--rounds' ? 1 : 0)) throw new Error(`${arg} takes a count`);
			if (arg === '--rounds') rounds = n;
			else warmup = n;
		} else if (arg.startsWith('--')) throw new Error(`${arg} is not an option: --rounds N, --warmup N, --gc, --json`);
		else dirs.push(arg);
	}
	const fallback = dirs.length === 0;
	if (fallback) dirs.push(new URL('../../crates/teasel/tests/hosts/svelte/', import.meta.url).pathname);

	// the same component copied across packages, or a family of icons, would weigh as many times as it appears
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

	// only what both accept is timed: a parser that stops at the first error would look fast on what it rejects
	const internal = await svelteInternal();
	const { parse: sveltePublic } = await import('svelte/compiler');
	const { parse, parseModule } = await import('../src/index.ts');
	const { load } = await import('../dist/engine/addon.js');
	const accepts = (f: () => unknown) => {
		try {
			f();
			return true;
		} catch {
			return false;
		}
	};
	let svelteRejected = 0;
	let teaselRejected = 0;
	const docs: Input[] = [];
	const scripts: Input[] = [];
	const templates: Input[] = [];
	let styles = 0;
	let styleBytes = 0;
	for (const text of unique) {
		const svelteOk = accepts(() => internal(text));
		const teaselOk = accepts(() => parse(text, { comments: true }));
		if (!svelteOk) svelteRejected++;
		if (!teaselOk) teaselRejected++;
		if (!svelteOk || !teaselOk) continue;
		// Svelte's tree tells where the scripts and the style sit
		const ast: Any = sveltePublic(text, { modern: true });
		const cuts: [number, number][] = [];
		let typescript = false;
		for (const script of [ast.module, ast.instance]) {
			if (!script) continue;
			const ts = /\blang\s*=\s*["']?ts\b/.test(text.slice(script.start, script.content.start));
			typescript ||= ts;
			scripts.push([text.slice(script.content.start, script.content.end), ts]);
			cuts.push([script.start, script.end]);
		}
		if (ast.css) {
			styles++;
			styleBytes += Buffer.byteLength(text.slice(ast.css.start, ast.css.end));
			cuts.push([ast.css.start, ast.css.end]);
		}
		let template = text;
		for (const [start, end] of cuts.sort((a, b) => b[0] - a[0])) template = template.slice(0, start) + template.slice(end);
		docs.push([text, typescript]);
		templates.push([template, typescript]);
	}
	if (docs.length === 0) throw new Error('no file both parsers accept');
	// the pieces cut out may not stand alone: a script whose exports name template bindings, a template whose TypeScript lost its script
	const scriptsOk = scripts.filter(([text, typescript]) => accepts(() => parseModule(text, { comments: true, typescript })));
	const templatesOk = templates.filter(([text, typescript]) => accepts(() => parse(text, { comments: true, typescript })));

	const time = (job: Job, inputs: Input[]) =>
		new Promise<Answer>((resolve, reject) => {
			// --no-addons carries over with the parent's flags; the fallback warning was shown once here already
			const child = fork(new URL(import.meta.url).pathname, ['--child'], { execArgv: [...process.execArgv, ...(gc ? ['--expose-gc'] : []), '--no-warnings'] });
			child.once('message', (answer: Answer) => resolve(answer));
			child.once('exit', (code) => (code === 0 ? undefined : reject(new Error(`${job} exited with ${code}`))));
			child.send({ job, inputs, rounds, warmup, gc } satisfies Request);
		});
	const stats = (values: number[]) => {
		const sorted = [...values].sort((a, b) => a - b);
		return { p10: sorted[Math.floor((sorted.length - 1) * 0.1)], median: sorted[Math.floor((sorted.length - 1) / 2)] };
	};
	const row = (answer: Answer, timesEngine: boolean) => {
		const total = stats(answer.totals);
		if (!timesEngine) return { total };
		const engine = stats(answer.engines);
		const decode = stats(answer.totals.map((t, i) => t - answer.engines[i]));
		return { total, engine, decode, decodeShare: decode.median / total.median };
	};
	const rows = {
		svelteInternal: row(await time('svelte', docs), false),
		sveltePublic: row(await time('public', docs), false),
		teasel: row(await time('teasel', docs), true),
		teaselScopes: row(await time('scopes', docs), true),
	};
	const speedup = (teasel: typeof rows.teasel) => ({
		overInternal: { p10: rows.svelteInternal.total.p10 / teasel.total.p10, median: rows.svelteInternal.total.median / teasel.total.median },
		overPublic: { p10: rows.sveltePublic.total.p10 / teasel.total.p10, median: rows.sveltePublic.total.median / teasel.total.median },
	});
	const size = (inputs: Input[]) => inputs.reduce((n, [text]) => n + Buffer.byteLength(text), 0);
	const piece = (inputs: Input[], answer: ReturnType<typeof row>) => ({ files: inputs.length, bytes: size(inputs), typescript: inputs.filter(([, ts]) => ts).length, ...answer });
	// loading again warns again, and the import warned already
	const emitWarning = process.emitWarning;
	process.emitWarning = () => {};
	const native = load() !== undefined;
	process.emitWarning = emitWarning;
	const result = {
		engine: native ? 'native' : 'wasm',
		node: process.version,
		svelte: JSON.parse(readFileSync(new URL('package.json', svelteDir), 'utf8')).version as string,
		rounds,
		warmup,
		gc,
		corpus: {
			fallback,
			found: paths.length,
			unique: unique.length,
			svelteRejected,
			teaselRejected,
			timed: docs.length,
			bytes: size(docs),
			typescript: docs.filter(([, ts]) => ts).length,
			styles,
			styleBytes,
		},
		rows,
		speedup: { teasel: speedup(rows.teasel), teaselScopes: speedup(rows.teaselScopes) },
		// where the engine's time goes: the documents, their scripts alone as plain modules, and the documents without script or style
		breakdown: {
			documents: piece(docs, rows.teasel),
			scripts: { ...piece(scriptsOk, row(await time('scripts', scriptsOk), true)), rejected: scripts.length - scriptsOk.length },
			templates: { ...piece(templatesOk, row(await time('templates', templatesOk), true)), rejected: templates.length - templatesOk.length },
		},
	};
	if (json) {
		console.log(JSON.stringify(result, null, '\t'));
		return;
	}

	const ms = (n: number) => n.toFixed(2).padStart(9);
	const kb = (n: number) => `${(n / 1024).toFixed(1)} kB`;
	const pct = (n: number) => `${(100 * n).toFixed(0)}%`.padStart(6);
	const x = (s: { p10: number; median: number }) => `${s.median.toFixed(2)}x (p10 ${s.p10.toFixed(2)}x)`;
	const c = result.corpus;
	console.log(fallback ? `corpus: the Svelte host tests, ${c.found} files; too few and too small to stand for real components, pass directories of them` : `corpus: ${dirs.join(' ')}`);
	console.log(`  ${c.found} .svelte files, ${c.unique} unique; Svelte rejected ${c.svelteRejected}, teasel ${c.teaselRejected}; ${c.timed} timed`);
	console.log(`  ${kb(c.bytes)} in all, ${kb(c.bytes / c.timed)} on average; ${c.typescript} in TypeScript, ${c.styles} with a style of ${kb(c.styleBytes)} in all`);
	console.log(`engine: ${result.engine === 'native' ? 'native addon' : 'WebAssembly'}; Node ${result.node}, svelte ${result.svelte}; ${rounds} rounds after ${warmup} warm-up${gc ? ', a full collection before each' : ''}, each row in its own process`);
	console.log(`\n${''.padEnd(30)}${'p10 ms'.padStart(9)}${'median ms'.padStart(11)}${'engine ms'.padStart(11)}${'decode ms'.padStart(11)}${'decode'.padStart(8)}`);
	const line = (label: string, r: ReturnType<typeof row>) =>
		console.log(`${label.padEnd(30)}${ms(r.total.p10)}  ${ms(r.total.median)}` + (r.engine ? `  ${ms(r.engine.median)}  ${ms(r.decode!.median)}  ${pct(r.decodeShare!)}` : ''));
	line('svelte internal parse', rows.svelteInternal);
	line('svelte parse, modern', rows.sveltePublic);
	line('teasel, comments', rows.teasel);
	line('teasel, comments and scopes', rows.teaselScopes);
	console.log(`\nteasel over svelte internal: ${x(result.speedup.teasel.overInternal)}; over svelte parse: ${x(result.speedup.teasel.overPublic)}`);
	console.log(`teasel with scopes over svelte internal: ${x(result.speedup.teaselScopes.overInternal)}; over svelte parse: ${x(result.speedup.teaselScopes.overPublic)}`);
	console.log(`\nwhere the engine's time goes (medians)${''.padEnd(5)}${'files'.padStart(6)}${'size'.padStart(11)}${'engine ms'.padStart(11)}${'decode ms'.padStart(11)}`);
	const part = (label: string, p: ReturnType<typeof piece> & { rejected?: number }) =>
		console.log(`${label.padEnd(43)}${String(p.files).padStart(6)}${kb(p.bytes).padStart(11)}  ${ms(p.engine!.median)}  ${ms(p.decode!.median)}` + (p.rejected ? `  (${p.rejected} rejected alone)` : ''));
	part('  whole documents', result.breakdown.documents);
	part('  scripts alone, as modules', result.breakdown.scripts);
	part('  documents without script or style', result.breakdown.templates);
}
