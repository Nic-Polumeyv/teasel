let a = 1;
function f(b) {
	let c = a + b;
	return () => c;
}
{
	let a = 2;
	a++;
}
var d;
d = 1;
for (let e of f) { e; }
try {} catch (g) { g; }
class H { static #i; j() { return H.#i; } }
import.meta;
k;
