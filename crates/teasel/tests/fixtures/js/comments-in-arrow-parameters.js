const a = (/* first */ x, ...rest) => x;
const b = (/* only */ ...rest) => rest;
const c = (x, /* second */ y,) => y;
const d = async (/* async */ x, ...rest) => x;
const e = (/** @type {number} */ x, [y] = [], ...rest) => x;
const f = (// line
	x, ...rest) => x;
