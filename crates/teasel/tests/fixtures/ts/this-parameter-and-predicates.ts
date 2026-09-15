function f(this: Window, x: unknown): x is string { return true; }
function g(this: void): asserts this is A {}
const h = function (this: any) {};
