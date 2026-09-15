function optionalDefaultRest(x?: number, y = 1, ...rest: number[]) {}
function receiver(this: object, x?: number) {}
function f(x = 1, y: number) {}
function overloaded(x?: number): void;
function overloaded(x?: number) {}
interface I {
    method(x?: number): void;
    [key: string]: unknown;
}
type T = (x?: number) => void;
const arrow = (x?: number) => x;
class C {
    constructor(public value: number) {}
    set x(value: number) {}
    [key: string]: unknown;
}
