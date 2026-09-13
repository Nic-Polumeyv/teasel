import {
	type Plan, type Form, type Mode, type Slot, type Dispatch,
	rule, seq, choice, optional, read, emit, token, js, get, constant, equal,
	scope, incoming, region, declare, array, concat, filter, map,
	and, or, not, any, isType, member, hasAttribute, present, test,
} from '../../../package/plan.js';

const rules: Plan['rules'] = {};
rules.Text = rule('Text', { content: 'null' }).form((f) => emit(f.content, get('event', 'decoded')));
rules.Comment = rule('Comment', { content: 'null' }).form((f) => emit(f.content, get('event', 'data')));
rules.Document = rule('Root', { children: 'null' })
	.form((f) => read({ kind: 'html-children', mode: 'normal', stop: { documentEnd: true } }, f.children))
	.regions((f) => [region('template', constant(null), [f.children], 'module')]);
rules.Interpolation = rule('Interpolation', { content: 'null' })
	.form((f) => seq(token('{{'), js('expression', f.content), token('}}')));

const element = (type: string, mode: Mode = 'normal', staticAttributes = false) =>
	rule(type, { tag: 'null', props: 'null', children: 'null' })
		.form((f) => seq(
			emit(f.tag, get('event', 'name')),
			read({ kind: 'html-attributes', mode: staticAttributes ? 'static' : 'normal' }, f.props),
			read({ kind: 'html-children', mode, stop: { matchingElement: true } }, f.children),
		))
		.regions((f) => {
			const vueDirective = (name: string) => filter(f.props, '$directive',
				and(isType(get('$directive'), 'Directive'), equal(get('$directive', 'name'), constant(name))));
			const templateSlot = and(isType(get('record'), 'Template'), any(vueDirective('slot')));
			return [
				region('loop', incoming, concat(
					filter(f.props, '$prop', not(and(isType(get('$prop'), 'Directive'),
						or(equal(get('$prop', 'name'), constant('for')), and(not(templateSlot), member(get('$prop', 'name'), ['if', 'else-if'])))))),
					array(f.children),
				), 'block', any(vueDirective('for'))),
				region('slot', scope('loop'), concat(map(vueDirective('slot'), '$slot', get('$slot', 'props')), array(f.children)),
					'function', any(vueDirective('slot'))),
			];
		});
rules.Element = element('Element');
rules.VerbatimElement = element('Element', 'verbatim', true);
rules.RawElement = element('Element', 'raw');
rules.RcdataElement = element('Element', 'rcdata');

type DirectiveSlots = { name: Slot<'name'>; arg: Slot<'arg'>; modifiers: Slot<'modifiers'>; rawName: Slot<'rawName'> };
const directiveFields = { name: 'null', arg: 'null', modifiers: 'null', rawName: 'null' } as const;
const directiveName = (f: DirectiveSlots, name?: string, prop = false) => seq(
	emit(f.name, name ? constant(name) : get('event', 'name')),
	emit(f.rawName, get('event', 'rawName')),
	emit(f.modifiers, prop ? concat(get('event', 'modifiers'), array(constant('prop'))) : get('event', 'modifiers')),
	choice(
		seq(test(and(present(get('event', 'argument')), get('event', 'argument', 'dynamic'))),
			js('expression', f.arg, get('event', 'argument'))),
		seq(test(not(and(present(get('event', 'argument')), get('event', 'argument', 'dynamic')))),
			emit(f.arg, get('event', 'argument', 'text'))),
	),
);
const optionalValue = <F extends Form<string, unknown>>(form: F) => choice(
	seq(read({ kind: 'test', value: present(get('event', 'value')) }), form),
	read({ kind: 'test', value: not(present(get('event', 'value'))) }),
);
const aliases = (f: { value: Slot<'value'>; key: Slot<'key'>; index: Slot<'index'> }) => seq(
	optional(js('pattern', f.value)),
	optional(seq(token(','), optional(js('pattern', f.key)),
		optional(seq(token(','), optional(js('pattern', f.index)))))),
);
rules.ForValue = rule('ForAliases', { value: 'null', key: 'null', index: 'null', source: 'null' }).form((f) => seq(
	choice(seq(token('('), aliases(f), token(')')), aliases(f)),
	choice(token('in'), token('of')), js('expression', f.source),
));
rules.For = rule('Directive', { ...directiveFields, value: 'null', key: 'null', index: 'null', source: 'null' }, ['parsed'])
	.form((f) => seq(
		directiveName(f, 'for'),
		read({ kind: 'rule', name: 'ForValue' }, f.parsed, get('event', 'value')),
		emit(f.value, get(f.parsed, 'value')), emit(f.key, get(f.parsed, 'key')),
		emit(f.index, get(f.parsed, 'index')), emit(f.source, get(f.parsed, 'source')),
	))
	.declares((f) => [declare([f.value, f.key, f.index], get('owner', 'scopes', 'loop'))]);
rules.SlotValue = rule('SlotProps', { props: 'null' })
	.form((f) => seq(optional(js('pattern', f.props)), read({ kind: 'space', min: 0 })));
rules.ExpressionValue = rule('DirectiveExpression', { exp: 'null' })
	.form((f) => seq(optional(js('expression', f.exp)), read({ kind: 'space', min: 0 })));
rules.SlotDirective = rule('Directive', { ...directiveFields, props: 'null' }, ['parsed'])
	.form((f) => seq(
		directiveName(f, 'slot'), optionalValue(seq(
			read({ kind: 'rule', name: 'SlotValue' }, f.parsed, get('event', 'value')),
			emit(f.props, get(f.parsed, 'props')),
		)),
	))
	.declares((f) => [declare([f.props], get('owner', 'scopes', 'slot'), 'param')]);
rules.On = rule('Directive', { ...directiveFields, handler: 'null' }).form((f) => seq(
	directiveName(f, 'on'), optionalValue(choice(
		js('expression', f.handler, get('event', 'value')),
		js('program', f.handler, get('event', 'value')),
	)),
));
for (const [key, name, prop] of [['Directive', undefined, false], ['Bind', 'bind', false], ['Prop', 'bind', true]] as const) {
	rules[key] = rule('Directive', { ...directiveFields, exp: 'null' }, ['parsed']).form((f) => seq(
		directiveName(f, name, prop), optionalValue(seq(
			read({ kind: 'rule', name: 'ExpressionValue' }, f.parsed, get('event', 'value')),
			emit(f.exp, get(f.parsed, 'exp')),
		)),
	));
}

const nameIs = (name: string) => equal(get('event', 'name'), constant(name));
const elementKinds: Dispatch[] = [
	{ when: nameIs('slot'), rule: 'Element', type: 'Slot' },
	{ when: nameIs('template'), rule: 'Element', type: 'Template' },
	{ when: nameIs('component'), rule: 'Element', type: 'Component' },
	{ when: member(get('event', 'name'), ['textarea', 'title']), rule: 'RcdataElement', type: 'Element', content: 'rcdata' },
	{ when: member(get('event', 'name'), ['script', 'style']), rule: 'RawElement', type: 'Element', content: 'raw' },
	{ when: or(and(get('event', 'nameFacts', 'uppercaseInitial'), get('event', 'nameFacts', 'identifier')),
		get('event', 'nameFacts', 'dottedIdentifier')), rule: 'Element', type: 'Component' },
	{ when: constant(true), rule: 'Element', type: 'Element' },
];
const elements: Dispatch[] = elementKinds.map((row) => ({
	when: and(hasAttribute(get('event'), 'v-pre'), row.when),
	rule: 'VerbatimElement', type: row.type, attributes: 'static', content: 'verbatim',
}));
elements.push(...elementKinds);
export const vue = {
	version: 1, document: 'Document', rules,
	html: {
		delimiters: ['{{', '}}'], attributeInterpolations: false, attributeComments: 'none',
		autoclose: false, trimEnd: false,
		void: ['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'],
		text: 'Text', comment: 'Comment',
		content: [{ prefix: '{{', rule: 'Interpolation' }], attribute: [],
		plainAttribute: { type: 'Attribute', name: 'name', value: 'value', text: 'Text', expression: 'Interpolation' },
		elements,
		directiveNames: { prefix: 'v-', argument: ':', modifier: '.', requireArgument: false, dynamic: ['[', ']'], unknown: 'wildcard-rule' },
		directives: [
			{ name: 'for', rule: 'For' }, { name: 'slot', rule: 'SlotDirective' },
			{ name: 'on', rule: 'On' },
			{ name: ':', rule: 'Bind' }, { name: '@', rule: 'On' },
			{ name: '#', rule: 'SlotDirective' }, { name: '.', rule: 'Prop' },
			{ name: '*', rule: 'Directive' },
		],
	},
} satisfies Plan;
