export function f(a: number, { b }?: { b: number }): void;
export function f(a: number, b?: any) {}
class C {
	m({ b }?: { b: number }): void;
	m(b?: any) {}
}
