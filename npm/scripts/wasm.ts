// node scripts/wasm.ts: the WebAssembly module, optimized, into dist/; `cargo build --release -p teasel-wasm --target wasm32-unknown-unknown` first
import { mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { target } from './target.ts';

mkdirSync('dist', { recursive: true });
// binaryen given --all-features rewrote calls into typed function references and tail calls, which need Safari 18.2 or Node 22, so it gets the six features rustc 1.98 emits
const features = ['bulk-memory', 'multivalue', 'mutable-globals', 'nontrapping-float-to-int', 'reference-types', 'sign-ext'].map((feature) => `--enable-${feature}`);
const run = spawnSync('wasm-opt', ['-Oz', ...features, `${target}/wasm32-unknown-unknown/release/teasel_wasm.wasm`, '-o', 'dist/teasel.wasm'], { stdio: 'inherit' });
if (run.error) throw run.error;
if (run.status !== 0) process.exit(run.status ?? 1);
