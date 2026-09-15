import type { A } from 'a';
import { type B, C } from 'b';
import type D from 'd';
type E = string;
export type { E };
type F = number;
const G = 1;
export { type F, G };
export type H = string;
import I = require('i');
import J = K.L;
export = M;
export as namespace N;
import type * as O from 'o';
export default interface Q {}
export import r = N.y;
import type s = require('s');
