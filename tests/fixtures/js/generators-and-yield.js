function* g() {
	const x = yield;
	yield x;
	yield* g();
	const y = yield a, b;
}
