import type { Engine } from '../types.ts';
import { instantiate } from './wasm.ts';

export const engine: Engine = await instantiate();
