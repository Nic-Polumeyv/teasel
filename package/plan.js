// Authoring a host plan: data for `Plan::read` in Rust, typed so that a misuse fails to compile.
// `plan.js` and `plan.d.ts` are emitted from this file.
const data = (value) => value;
// the name rides along for `into`, hidden from JSON because Rust's reader knows only op/base/path
const slot = (base, name) => data(Object.defineProperty({ op: 'get', base, path: [name] }, 'name', { value: name }));
const list = (value) => (Array.isArray(value) ? array(...value) : value);
export const constant = (value) => data({ op: 'constant', value });
export const get = (base, ...path) => data({ op: 'get', base, path });
export const equal = (left, right) => data({ op: 'compare', relation: 'equal', left, right });
export const less = (left, right) => data({ op: 'compare', relation: 'less', left, right });
export const present = (left) => data({ op: 'compare', relation: 'present', left });
export const choose = (condition, yes, no) => data({ op: 'choose', condition, yes, no });
export const flatMap = (list, as, body) => data({ op: 'flatMap', list, as, body });
export const length = (list) => data({ op: 'length', list });
export const at = (list, index) => data({ op: 'at', list, index: constant(index) });
export const array = (...items) => data({ op: 'construct', shape: 'array', items });
export const record = (type, fields, span = constant(null)) => data({ op: 'construct', shape: 'record', type, fields, span });
export const not = (p) => choose(p, constant(false), constant(true));
export const and = (a, b) => choose(a, b, constant(false));
export const or = (a, b) => choose(a, constant(true), b);
export const filter = (list, as, predicate) => flatMap(list, as, choose(predicate, array(get(as)), array()));
export const map = (list, as, value) => flatMap(list, as, array(value));
export const any = (list) => less(constant(0), length(list));
export const member = (value, literals) => any(filter(constant(literals), '$candidate', equal(get('$candidate'), value)));
export const concat = (...lists) => flatMap(array(...lists), '$list', get('$list'));
export const scope = (name) => get('scopes', name);
export const incoming = get('incoming');
export const isType = (value, ...types) => member(get(value, 'type'), types);
export const attributes = (node) => choose(present(get(node, 'header', 'attributes')), get(node, 'header', 'attributes'), array());
export const attr = (node, name) => filter(attributes(node), '$attribute', and(equal(get('$attribute', 'kind'), constant('ordinary')), equal(get('$attribute', 'name'), constant(name))));
export const hasAttribute = (node, name) => any(attr(node, name));
export const staticAttribute = (node, name) => get(at(attr(node, name), 0), 'staticText');
export const seq = (...items) => data({ op: 'seq', items });
export const choice = (...alternatives) => data({ op: 'choice', alternatives });
export const optional = (form) => data(choice(form, seq()));
export const read = (reader, into, input) => data({ op: 'read', reader, ...(into && { into: into.name }), ...(input && { input }) });
export const emit = (into, value) => data({ op: 'emit', into: into.name, value });
export const token = (text, tight = false) => read({ kind: 'token', text, gap: tight ? 'none' : 'space*', word: /[A-Za-z0-9_]$/.test(text) });
export const space = () => read({ kind: 'space', min: 1 });
export const test = (value) => read({ kind: 'test', value });
export const call = (name, into) => read({ kind: 'rule', name }, into);
export const js = (entry, into, input, boundary) => read({ kind: 'javascript', entry, ...(boundary && { boundary }) }, into, input);
const item = slot('iteration', 'item');
export const repeat = (build, into, value = item, min = 0, max = null) => data({ op: 'repeat', body: build({ item }), min, max, locals: ['item'], yield: value, into: into.name });
export const many = (name, into, min, max) => repeat((r) => call(name, r.item), into, item, min, max);
export const region = (id, parent, covers, kind = 'fragment', when) => data({ id, parent, kind, covers: list(covers), ...(when && { when }) });
export const declare = (patterns, into, kind = 'pattern') => data({
    patterns: list(patterns),
    into: typeof into === 'string' ? (into === 'incoming' ? incoming : scope(into)) : into,
    kind,
});
// a chain, not one call with an options object: TypeScript fixes W before a deferred form callback runs
export class Rule {
    #data;
    #slots;
    constructor(type, fields, locals) {
        this.#data = { type, fields, ...(locals.length && { locals: [...locals] }), form: seq() };
        this.#slots = data(Object.fromEntries([
            ...Object.keys(fields).map((name) => [name, slot('record', name)]),
            ...locals.map((name) => [name, slot('locals', name)]),
        ]));
    }
    form(build) {
        this.#data.form = build(this.#slots);
        return data(this);
    }
    regions(build) {
        this.#data.regions = [...(this.#data.regions ?? []), ...build(this.#slots)];
        return data(this);
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
export const rule = (type, fields, locals) => new Rule(type, fields, locals ?? data([]));
