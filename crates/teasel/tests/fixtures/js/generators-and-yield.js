function* g() {
	const x = yield;
	yield x;
	yield* g();
	yield a, b;
}
