// node scripts/wasm.ts: the WebAssembly module, optimized, into dist/; `cargo build --release -p teasel-wasm --target wasm32-unknown-unknown` first
import { mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { target } from './target.ts';

mkdirSync('dist', { recursive: true });
const run = spawnSync('wasm-opt', ['-Oz', '--all-features', `${target}/wasm32-unknown-unknown/release/teasel_wasm.wasm`, '-o', 'dist/teasel.wasm'], { stdio: 'inherit' });
if (run.error) throw run.error;
if (run.status !== 0) process.exit(run.status ?? 1);
