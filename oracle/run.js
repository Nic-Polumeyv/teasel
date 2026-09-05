// Differential test: acorn versus teasel over every script in a Svelte checkout.
//
//   SVELTE_DIR=~/Projects/svelte bun run.js [--verbose] [--limit N] [filter]

import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import { acorn_parse, args, compare, corpus, files, teasel } from './lib.js';

const { verbose, limit, filter } = args();
const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;

const jobs = [];
for (const path of files(corpus, /\.(svelte|js)$/)) {
	const name = relative(corpus, path);
	if (filter && !name.includes(filter)) continue;
	const text = readFileSync(path, 'utf8');
	if (path.endsWith('.js')) {
		jobs.push({ name, source: text, mode: 'module' });
		if (!/^\s*(import|export)\b/m.test(text)) jobs.push({ name: `${name} (script)`, source: text, mode: 'script' });
	} else {
		for (const match of text.matchAll(script_re)) {
			if (/lang=["']?ts/.test(match[1] ?? '')) continue;
			jobs.push({ name: `${name}#${match.index}`, source: match[2], mode: 'module' });
		}
	}
	if (jobs.length >= limit) {
		jobs.length = limit;
		break;
	}
}

const lines = await teasel(jobs);
process.exit(compare(jobs, (job) => acorn_parse(job.source, job.mode), lines, { verbose }) ? 0 : 1);
