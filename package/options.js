// The request's switches as bits, bit `i` being `FLAGS[i]` of `json.rs`, from options spelled
// as acorn's. `script` follows from `sourceType` and `until` belongs to the call, so neither is
// an option.
const FLAGS = ['typescript', 'comments', 'scopes', 'locations', 'script', 'preserveParens', 'allowReturnOutsideFunction', 'allowAwaitOutsideFunction', 'allowSuperOutsideMethod', 'allowUndeclaredExports', 'untilAs', 'erase'];
const BIT = Object.fromEntries(FLAGS.map((flag, i) => [flag, 1 << i]));
const DERIVED = new Set(['script', 'untilAs']);

/** Rejects the acorn options teasel cannot honor and the values it cannot take. */
export function bits(options) {
	if (options === undefined) return BIT.script;
	for (const key of ['ranges', 'onComment', 'onToken', 'onInsertedSemicolon', 'onTrailingComma']) {
		if (key in options) throw new TypeError(`the ${key} option is not supported`);
	}
	if ('sourceType' in options && options.sourceType !== 'script' && options.sourceType !== 'module') {
		throw new TypeError(`sourceType must be "script" or "module", not ${JSON.stringify(options.sourceType)}`);
	}
	if ('until' in options && options.until !== 'as') {
		throw new TypeError(`until must be "as", not ${JSON.stringify(options.until)}`);
	}
	let on = options.sourceType === 'module' ? 0 : BIT.script;
	for (const flag of FLAGS) if (!DERIVED.has(flag) && options[flag] === true) on |= BIT[flag];
	if (options.typescript === 'erase') on |= BIT.typescript | BIT.erase;
	return on;
}
