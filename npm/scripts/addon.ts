// node scripts/addon.ts [--target TRIPLE]: builds the addon for this machine, or for the target given, into teasel.<platform>.node
import { copyFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { here, platforms } from '../lib/addon.js';

const at = process.argv.indexOf('--target');
const wanted = at === -1 ? platforms[here]?.target : process.argv[at + 1];
const found = Object.entries(platforms).find(([, p]) => p.target === wanted);
if (found === undefined) throw new Error(`no platform builds for ${wanted}`);
const [tag, { target, os }] = found;

const root = fileURLToPath(new URL('../../', import.meta.url));
const run = spawnSync('cargo', ['build', '--release', '-p', 'teasel-node', '--target', target], { stdio: 'inherit', cwd: root });
if (run.error) throw run.error;
if (run.status !== 0) process.exit(run.status ?? 1);
const lib = os === 'win32' ? 'teasel_node.dll' : os === 'darwin' ? 'libteasel_node.dylib' : 'libteasel_node.so';
copyFileSync(`${root}target/${target}/release/${lib}`, `teasel.${tag}.node`);
