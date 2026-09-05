// Differential test over regular expression literals taken from acorn's own regexp tests
// (oracle/regexp-cases.json): each is parsed as a script by both parsers.
//
//   bun regexp.js [--verbose] [filter]

import { readFileSync } from 'node:fs';
import { acorn_parse, args, compare, teasel } from './lib.js';

const { verbose, filter } = args();
const cases = JSON.parse(readFileSync(new URL('./regexp-cases.json', import.meta.url), 'utf8'));
const jobs = cases.filter((c) => !filter || c.includes(filter)).map((source) => ({ name: source, source, mode: 'script' }));
const expected = jobs.map((job) => acorn_parse(job.source, job.mode));
const actual = await teasel(jobs);
process.exit(compare(jobs, expected, actual, { verbose, label: 'regexp-cases.json' }) ? 0 : 1);
