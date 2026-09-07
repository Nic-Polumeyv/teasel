// Differential test: acorn versus teasel over every script in a Svelte checkout.
//
//   SVELTE_DIR=~/Projects/svelte bun run.js [--verbose] [--limit N] [filter]

import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import { acorn_parse, args, capped, compare, corpus, files, scripts, teasel } from './lib.js';

const { verbose, limit, filter } = args();

const jobs = [];
for (const path of files(corpus, /\.(svelte|js)$/)) {
	const name = relative(corpus, path);
	if (filter && !name.includes(filter)) continue;
	const text = readFileSync(path, 'utf8');
	if (path.endsWith('.js')) {
		jobs.push({ name, source: text, mode: 'module' });
		if (!/^\s*(import|export)\b/m.test(text)) jobs.push({ name: `${name} (script)`, source: text, mode: 'script' });
	} else {
		for (const { index, source } of scripts(text, false)) jobs.push({ name: `${name}#${index}`, source, mode: 'module' });
	}
	if (capped(jobs, limit)) break;
}

const lines = await teasel(jobs);
process.exit(compare(jobs, (job) => acorn_parse(job.source, job.mode), lines, { verbose }) ? 0 : 1);
