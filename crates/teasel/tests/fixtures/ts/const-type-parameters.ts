class C<const T> {}
const D = class<const in T> {};
type F = <const T>(x: T) => T;
interface I {
	m<const T>(x: T): T;
	<const T>(x: T): T;
	new <const T>(x: T): T;
}
type N = new <const T>(x: T) => T;
function f<const T>() {}
