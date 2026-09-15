// node scripts/addon.ts [--target TRIPLE]: builds the addon for this machine, or for the target given, into teasel.<platform>.node
import { copyFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { here, platforms } from '../lib/native.js';

const at = process.argv.indexOf('--target');
const wanted = at === -1 ? platforms[here]?.target : process.argv[at + 1];
const found = Object.entries(platforms).find(([, p]) => p.target === wanted);
if (found === undefined) throw new Error(`no platform builds for ${wanted}`);
const [tag, { target, os }] = found;

const { status } = spawnSync('cargo', ['build', '--release', '-p', 'teasel-node', '--target', target], { stdio: 'inherit', cwd: '..' });
if (status !== 0) process.exit(status ?? 1);
const lib = os === 'win32' ? 'teasel_node.dll' : os === 'darwin' ? 'libteasel_node.dylib' : 'libteasel_node.so';
copyFileSync(`../target/${target}/release/${lib}`, `teasel.${tag}.node`);
