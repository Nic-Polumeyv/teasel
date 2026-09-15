function f<T, U extends keyof T = keyof T>(t: T, u: U): T[U] { return t[u]; }
const g = <T,>(x: T) => x;
class C<in out T> {}
f<string, 'length'>('x', 'length');
new Map<string, number>();
a?.<T>();
b.c<D>();
let deep: A<B<C<D>>> = 1;
let fe = function <T>() {};
const cf = <const T,>(x: T) => x;
let call = f<T>(y);
let lt = a < b > c;
let shift = a<b>>c;
let inst = new Foo<T>();
let tagged = f<T>`tpl`;
let instantiation = A<B>;
