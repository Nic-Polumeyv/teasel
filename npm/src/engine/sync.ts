import type { Engine } from '../types.ts';
import { compile } from './wasm.ts';

export const engine: Engine = compile();
