type A = `${Uppercase<B>}-${number}`;
type C<T extends string> = T extends `${infer D}.${infer E}` ? [D, E] : never;
