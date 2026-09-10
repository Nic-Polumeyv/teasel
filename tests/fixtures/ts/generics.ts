function f<T, U extends keyof T = keyof T>(t: T, u: U): T[U] { return t[u]; }
const g = <T,>(x: T) => x;
class C<in out T> {}
f<string, 'length'>('x', 'length');
new Map<string, number>();
a?.<T>();
b.c<D>();
