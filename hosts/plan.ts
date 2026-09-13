export type JSONValue = null | boolean | number | string | JSONValue[] | { [key: string]: JSONValue };
export type Value =
  | { op: 'constant'; value: JSONValue }
  | { op: 'get'; base: string | V; path: (string | number)[] }
  | { op: 'compare'; relation: 'equal' | 'less' | 'present'; left: V; right?: V }
  | { op: 'choose'; condition: V; yes: V; no: V }
  | { op: 'flatMap'; list: V; as: string; body: V }
  | { op: 'length'; list: V }
  | { op: 'at'; list: V; index: V }
  | { op: 'construct'; shape: 'array'; items: V[] }
  | { op: 'construct'; shape: 'record'; type: string | null; fields: Record<string, V>; span: V };
export type V = Value;
export type F = Form;
export type JS = 'expression' | 'assignmentExpression' | 'pattern' | 'bindingIdentifier'
  | 'identifierReference' | 'params' | 'typeParameters' | 'statement' | 'program';
export type Stop = { prefixes: string[] } | { matchingElement: true } | { documentEnd: true };
export type Mode = 'normal' | 'raw' | 'rcdata' | 'verbatim';
export type Reader =
  | { kind: 'token'; text: string; gap: 'space*' | 'none'; word: boolean }
  | { kind: 'space'; min: number }
  | { kind: 'test'; value: V }
  | { kind: 'rule'; name: string }
  | { kind: 'javascript'; entry: JS; boundary?: 'last-shared-word' }
  | { kind: 'html-single'; entry: JS }
  | { kind: 'html-attributes'; mode: 'normal' | 'static' }
  | { kind: 'html-attribute-parts' }
  | { kind: 'html-children'; mode: Mode; stop: Stop }
  | { kind: 'css-stylesheet' };
export type Form =
  | { op: 'seq'; items: F[] }
  | { op: 'choice'; alternatives: F[] }
  | { op: 'repeat'; body: F; min: number; max: number | null;
      locals: string[]; yield: V; into: string }
  | { op: 'read'; reader: Reader; into?: string; input?: V }
  | { op: 'emit'; into: string; value: V };
export type Region = {
  id: string; parent: V; kind: 'module' | 'script' | 'fragment' | 'block' | 'function';
  covers: V; when?: V; each?: { list: V; as: string };
};
export type Rule = {
  type: string; fields: Record<string, 'null' | 'omit'>; locals?: string[]; form: F;
  regions?: Region[];
  declares?: Declare[];
  span?: 'none' | 'through-next-token-start';
};
export type Declare = { patterns: V; into: V; kind: 'pattern' | 'param' };
export type Html = Plan['html'];
export type Dispatch = { when: V; rule: string; attributes?: 'static'; content?: Mode };
export type Plan = {
  version: 1; document: string; rules: Record<string, Rule>;
  html: {
    delimiters: [string, string]; attributeInterpolations: boolean; attributeComments: 'javascript' | 'none';
    autoclose: boolean; trimEnd: boolean; void: string[]; text: string; comment: string;
    content: { prefix: string; rule: string }[]; attribute: { prefix: string; rule: string }[];
    plainAttribute: { type: string; name: string; value: string; text: string; expression: string };
    elements: Dispatch[];
    directiveNames: { prefix: string; argument: string; modifier: string; requireArgument: boolean;
      dynamic: [string, string] | null; unknown: 'plain-attribute' | 'wildcard-rule' };
    directives: { name: string; rule: string }[];
  };
};

export const constant = (value: JSONValue): V => ({ op: 'constant', value });
export const get = (base: string | V, ...path: (string | number)[]): V => ({ op: 'get', base, path });
export const equal = (left: V, right: V): V => ({ op: 'compare', relation: 'equal', left, right });
export const less = (left: V, right: V): V => ({ op: 'compare', relation: 'less', left, right });
export const present = (left: V): V => ({ op: 'compare', relation: 'present', left });
export const choose = (condition: V, yes: V, no: V): V => ({ op: 'choose', condition, yes, no });
export const flatMap = (list: V, as: string, body: V): V => ({ op: 'flatMap', list, as, body });
export const length = (list: V): V => ({ op: 'length', list });
export const at = (list: V, index: number): V => ({ op: 'at', list, index: constant(index) });
export const array = (...items: V[]): V => ({ op: 'construct', shape: 'array', items });
export const record = (type: string | null, fields: Record<string, V>, span = constant(null)): V =>
  ({ op: 'construct', shape: 'record', type, fields, span });
export const not = (p: V) => choose(p, constant(false), constant(true));
export const and = (a: V, b: V) => choose(a, b, constant(false));
export const or = (a: V, b: V) => choose(a, constant(true), b);
export const filter = (list: V, as: string, predicate: V) =>
  flatMap(list, as, choose(predicate, array(get(as)), array()));
export const map = (list: V, as: string, value: V) => flatMap(list, as, array(value));
export const any = (list: V) => less(constant(0), length(list));
export const member = (value: V, literals: JSONValue[]) => any(filter(constant(literals), '$candidate', equal(get('$candidate'), value)));
export const concat = (...lists: V[]) => flatMap(array(...lists), '$list', get('$list'));
export const field = (name: string) => get('record', name);
export const scope = (name: string) => get('scopes', name);
export const incoming = get('incoming');
export const isType = (value: V, ...types: string[]) => member(get(value, 'type'), types);
export const attributes = (node: V) => choose(present(get(node, 'header', 'attributes')), get(node, 'header', 'attributes'), array());
export const attr = (node: V, name: string) => filter(attributes(node), '$attribute',
  and(equal(get('$attribute', 'kind'), constant('ordinary')), equal(get('$attribute', 'name'), constant(name))));
export const hasAttribute = (node: V, name: string) => any(attr(node, name));
export const staticAttribute = (node: V, name: string) => get(at(attr(node, name), 0), 'staticText');

export const seq = (...items: F[]): F => ({ op: 'seq', items });
export const choice = (...alternatives: F[]): F => ({ op: 'choice', alternatives });
export const optional = (form: F) => choice(form, seq());
export const read = (reader: Reader, into?: string, input?: V): F => ({ op: 'read', reader, ...(into ? { into } : {}), ...(input ? { input } : {}) });
export const emit = (into: string, value: V): F => ({ op: 'emit', into, value });
export const token = (text: string, tight = false) => read({ kind: 'token', text, gap: tight ? 'none' : 'space*', word: /[A-Za-z0-9_]$/.test(text) });
export const space = () => read({ kind: 'space', min: 1 });
export const test = (value: V) => read({ kind: 'test', value });
export const call = (name: string, into?: string) => read({ kind: 'rule', name }, into);
export const js = (entry: JS, into: string, input?: V, boundary?: 'last-shared-word') =>
  read({ kind: 'javascript', entry, ...(boundary ? { boundary } : {}) }, into, input);
export const fields = (required: string[], omitted: string[] = []): Rule['fields'] =>
  Object.fromEntries([...required.map(n => [n, 'null']), ...omitted.map(n => [n, 'omit'])]);
export const rule = (type: string, names: string[], form: F, extra: Partial<Omit<Rule, 'type' | 'form'>> = {}): Rule =>
  ({ type, fields: fields(names), form, ...extra });
export const region = (id: string, parent: V, covers: V, kind: Region['kind'] = 'fragment', when?: V): Region =>
  ({ id, parent, kind, covers, ...(when ? { when } : {}) });
export const declare = (patterns: V, into: V, kind: 'pattern' | 'param' = 'pattern') => ({ patterns, into, kind });
export const repeat = (body: F, into: string, value: V, min = 0, max: number | null = null): F =>
  ({ op: 'repeat', body, min, max, locals: ['item'], yield: value, into });
export const many = (name: string, into: string, min: number, max: number | null) =>
  repeat(call(name, 'item'), into, get('iteration', 'item'), min, max);
