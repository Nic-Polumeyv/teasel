outer: for (const a of b) {
	inner: for (const c of d) {
		if (c) continue outer;
		break inner;
	}
}
block: { break block; }
