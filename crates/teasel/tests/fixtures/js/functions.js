function f(a, b = 2, ...c) {}
function* g() { yield; yield* h(); }
async function h() { for await (const x of y) {} }
const i = function named() {};
function j(this_, arguments_) { return arguments; }
