// Differential test over tc39/test262-parser-tests: acorn versus teasel on every file in
// pass, pass-explicit, fail and early. The comparison is always against acorn, not against the
// suite's own verdict; a separate line reports where acorn disagrees with the suite.
//
//   git clone --depth 1 https://github.com/tc39/test262-parser-tests.git
//   bun test262.js [--verbose] [filter]

import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { acorn_parse, args, compare, teasel } from './lib.js';

const { verbose, limit, filter } = args();
const root = new URL('./test262-parser-tests', import.meta.url).pathname;

const jobs = [];
for (const dir of ['pass', 'pass-explicit', 'fail', 'early']) {
	for (const name of readdirSync(join(root, dir)).sort()) {
		if (filter && !`${dir}/${name}`.includes(filter)) continue;
		const source = readFileSync(join(root, dir, name), 'utf8');
		const mode = name.includes('.module.') ? 'module' : 'script';
		jobs.push({ name: `${dir}/${name}`, source, mode, should_pass: dir.startsWith('pass') });
		if (jobs.length >= limit) break;
	}
	if (jobs.length >= limit) break;
}

const expected = (job) => acorn_parse(job.source, job.mode);
const lines = await teasel(jobs);

const acorn_wrong = jobs.filter((job) => Boolean(expected(job).error) === job.should_pass);
console.log(`acorn disagrees with the suite on ${acorn_wrong.length} files`);
if (verbose) for (const job of acorn_wrong) console.log(`  ${job.name}`);
process.exit(compare(jobs, expected, lines, { verbose, label: 'test262-parser-tests' }) ? 0 : 1);
