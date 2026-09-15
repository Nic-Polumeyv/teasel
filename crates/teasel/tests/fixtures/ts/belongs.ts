function isString(x: unknown): x is string { return typeof x === "string"; }
function assertString(x: unknown): asserts x is string {}
function assertPresent(x: unknown): asserts x {}
class C {
  isString(x: unknown): x is string { return typeof x === "string"; }
  isC(): this is C { return true; }
}
type Predicate = (x: unknown) => x is string;
type Assertion = (x: unknown) => asserts x;
interface Predicates {
  (x: unknown): x is string;
  (x: unknown): asserts x;
  isC(): this is C;
  assertString(x: unknown): asserts x is string;
}
type Inferred<T> = T extends infer U ? U : never;
type Template<T> = T extends `${infer U}` ? U : never;
type Tuple<T> = T extends [infer U, [infer V]] ? [U, V] : never;
const values = [
  "s" as const,
  1 as const,
  1n as const,
  true as const,
  false as const,
  `text` as const,
  `text${1}` as const,
  -1 as const,
  +1 as const,
  -1n as const,
  +1n as const,
  [1] as const,
  { a: 1 } as const,
  (1) as const,
  E.A as const,
  E["A"] as const,
  <const>[1]
];
try {} catch (e: any) {}
try {} catch (e: unknown) {}
enum E { A = 1, B = A + 1 }
declare module "m" {}
abstract class Abstract {
  abstract method(): void;
  abstract get value(): string;
  abstract set value(value: string);
}
