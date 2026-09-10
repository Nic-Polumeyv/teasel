type A<T> = { -readonly [K in keyof T as `get${Capitalize<K & string>}`]-?: () => T[K] };
type B<T> = T extends (...args: infer P) => infer R ? [P, R] : never;
