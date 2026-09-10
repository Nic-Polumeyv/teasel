type A = [string, number?, ...boolean[]];
type B = readonly [a: string, ...rest: number[]];
function f(...args: [string, number]) {}
