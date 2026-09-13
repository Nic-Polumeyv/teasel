// Constructors for a host plan. Every function returns plain data; the types live in plan.d.ts.

const slot = (base, name) => {
	const value = { op: 'get', base, path: [name] };
	Object.defineProperty(value, 'name', { value: name });
	return value;
};
const named = (value) => (typeof value === 'object' && value !== null && 'name' in value ? value.name : value);
const list = (value) => (Array.isArray(value) ? array(...value) : value);

export const constant = (value) => ({ op: 'constant', value });
export const get = (base, ...path) => ({ op: 'get', base, path });
export const equal = (left, right) => ({ op: 'compare', relation: 'equal', left, right });
export const less = (left, right) => ({ op: 'compare', relation: 'less', left, right });
export const present = (left) => ({ op: 'compare', relation: 'present', left });
export const choose = (condition, yes, no) => ({ op: 'choose', condition, yes, no });
export const flatMap = (list, as, body) => ({ op: 'flatMap', list, as, body });
export const length = (list) => ({ op: 'length', list });
export const at = (list, index) => ({ op: 'at', list, index: constant(index) });
export const array = (...items) => ({ op: 'construct', shape: 'array', items });
export const record = (type, fields, span = constant(null)) => ({ op: 'construct', shape: 'record', type, fields, span });
export const not = (p) => choose(p, constant(false), constant(true));
export const and = (a, b) => choose(a, b, constant(false));
export const or = (a, b) => choose(a, constant(true), b);
export const filter = (list, as, predicate) => flatMap(list, as, choose(predicate, array(get(as)), array()));
export const map = (list, as, value) => flatMap(list, as, array(value));
export const any = (list) => less(constant(0), length(list));
export const member = (value, literals) =>
	any(filter(constant(literals), '$candidate', equal(get('$candidate'), value)));
export const concat = (...lists) => flatMap(array(...lists), '$list', get('$list'));
export const scope = (name) => get('scopes', name);
export const incoming = get('incoming');
export const isType = (value, ...types) => member(get(value, 'type'), types);
export const attributes = (node) =>
	choose(present(get(node, 'header', 'attributes')), get(node, 'header', 'attributes'), array());
export const attr = (node, name) =>
	filter(
		attributes(node),
		'$attribute',
		and(equal(get('$attribute', 'kind'), constant('ordinary')), equal(get('$attribute', 'name'), constant(name))),
	);
export const hasAttribute = (node, name) => any(attr(node, name));
export const staticAttribute = (node, name) => get(at(attr(node, name), 0), 'staticText');

export const seq = (...items) => ({ op: 'seq', items });
export const choice = (...alternatives) => ({ op: 'choice', alternatives });
export const optional = (form) => choice(form, seq());
export const read = (reader, into, input) => ({
	op: 'read',
	reader,
	...(into ? { into: named(into) } : {}),
	...(input ? { input } : {}),
});
export const emit = (into, value) => ({ op: 'emit', into: named(into), value });
export const token = (text, tight = false) =>
	read({ kind: 'token', text, gap: tight ? 'none' : 'space*', word: /[A-Za-z0-9_]$/.test(text) });
export const space = () => read({ kind: 'space', min: 1 });
export const test = (value) => read({ kind: 'test', value });
export const call = (name, into) => read({ kind: 'rule', name }, into);
export const js = (entry, into, input, boundary) =>
	read({ kind: 'javascript', entry, ...(boundary ? { boundary } : {}) }, into, input);
const item = slot('iteration', 'item');
export const repeat = (build, into, value = item, min = 0, max = null) => ({
	op: 'repeat',
	body: build({ item }),
	min,
	max,
	locals: ['item'],
	yield: value,
	into: named(into),
});
export const many = (name, into, min, max) => repeat((r) => call(name, r.item), into, item, min, max);

export const region = (id, parent, covers, kind = 'fragment', when) => ({
	id,
	parent,
	kind,
	covers: list(covers),
	...(when ? { when } : {}),
});
export const declare = (patterns, into, kind = 'pattern') => ({
	patterns: list(patterns),
	into: typeof into === 'string' ? (into === 'incoming' ? incoming : scope(into)) : into,
	kind,
});

class Rule {
	#data;
	#slots;
	constructor(type, fields, locals) {
		this.#data = { type, fields, ...(locals.length ? { locals: [...locals] } : {}), form: seq() };
		const slots = Object.fromEntries(Object.keys(fields).map((name) => [name, slot('record', name)]));
		for (const name of locals) slots[name] = slot('locals', name);
		this.#slots = Object.freeze(slots);
	}
	form(build) {
		this.#data.form = build(this.#slots);
		return this;
	}
	regions(build) {
		this.#data.regions = [...(this.#data.regions ?? []), ...build(this.#slots)];
		return this;
	}
	declares(build) {
		this.#data.declares = [...(this.#data.declares ?? []), ...build(this.#slots)];
		return this;
	}
	span(policy) {
		this.#data.span = policy;
		return this;
	}
	toJSON() {
		return this.#data;
	}
}
export const rule = (type, fields, locals = []) => new Rule(type, fields, locals);
