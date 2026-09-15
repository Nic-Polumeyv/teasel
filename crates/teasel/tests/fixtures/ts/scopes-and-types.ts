import type { A } from 'a';
import { B } from 'b';
let c: A = B;
type D = typeof c;
function f<T>(t: T): T { return t; }
namespace N { export const g = 1; }
N.g;
enum E { X, Y = X }
declare const h: number;
h;
