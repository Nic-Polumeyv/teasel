import {
	type V, type Form, type JS, type Stop, type Mode, type Slot, type Dispatch, type Plan, constant, get,
	equal, present, choose, at, array, record, not, and, or, filter, map, any, member, concat, scope,
	incoming, isType, hasAttribute, staticAttribute, seq, choice, optional, read, emit, token, space, test,
	call, js, rule, region, declare, repeat, many,
} from '../../../../../npm/src/plan.ts';

const named = (node: V) => present(staticAttribute(node, 'slot'));
const component = (node: V) => isType(node, 'Component', 'SvelteComponent', 'SvelteSelf');
const header = <Fs extends readonly Form[]>(...items: Fs) => seq(token('{'), ...items, token('}'));
const end = (name: string) => header(token('/' + name));
const branch = <Fs extends readonly Form[]>(...items: Fs) => header(token(':'), ...items);
const body = <N extends string>(into: Slot<N>) => call('Body', into);
const event = get('event');
const attributeExpression = (attribute: V) => {
	const value = get(attribute, 'value');
	const part = choose(present(get(value, 1)), constant(null), get(value, 0));
	return choose(isType(value, 'ExpressionTag'), get(value, 'expression'), get(part, 'expression'));
};

const rules: Plan['rules'] = {};
rules.Text = rule('Text', { data: 'null', raw: 'null' })
	.form((f) => seq(emit(f.data, get('event', 'decoded')), emit(f.raw, get('event', 'raw'))));
rules.Comment = rule('Comment', { data: 'null' }).form((f) => emit(f.data, get('event', 'data')));
const templateNode = (node: V) => choose(isType(node, 'UnmarkedTag'),
	choose(present(get(node, 'declaration')),
		record('DeclarationTag', { declaration: get(node, 'declaration') }, get(node, 'span')),
		record('ExpressionTag', { expression: get(node, 'expression') }, get(node, 'span'))), node);
const children = (mode: Mode, stop: Stop) =>
	rule('Fragment', { nodes: 'null' }, ['children']).form((f) => seq(
		read({ kind: 'html-children', mode, stop }, f.children),
		emit(f.nodes, map(f.children, '$node', templateNode(get('$node')))),
	)).span('none');
rules.Body = children('normal', { prefixes: ['{:', '{/'] });
rules.DocumentChildren = children('normal', { documentEnd: true });
rules.NormalChildren = children('normal', { matchingElement: true });
rules.RawChildren = children('raw', { matchingElement: true });
rules.RcdataChildren = children('rcdata', { matchingElement: true });
const nodesOfType = (nodes: V, ...types: string[]) => filter(nodes, '$node', isType(get('$node'), ...types));
const scripts = (nodes: V, context: string) => filter(nodesOfType(nodes, 'Script'), '$script',
	equal(get('$script', 'context'), constant(context)));
rules.Document = rule('Root', {
	css: 'null', js: 'null', options: 'null', comments: 'null', module: 'omit', instance: 'omit', fragment: 'null',
}, ['children'])
	.form((f) => seq(
		call('DocumentChildren', f.children),
		emit(f.css, at(nodesOfType(get(f.children, 'nodes'), 'StyleSheet'), 0)),
		emit(f.js, array()), emit(f.comments, get('event', 'comments')),
		emit(f.module, at(scripts(get(f.children, 'nodes'), 'module'), 0)),
		emit(f.instance, at(scripts(get(f.children, 'nodes'), 'default'), 0)),
		emit(f.fragment, record('Fragment', { nodes: filter(get(f.children, 'nodes'), '$node',
			not(or(equal(get('$node'), f.css), or(equal(get('$node'), f.module), equal(get('$node'), f.instance))))) })),
	))
	.regions((f) => [
		region('module', constant(null), map(scripts(get(f.children, 'nodes'), 'module'), '$script', get('$script', 'content')), 'module'),
		region('instance', scope('module'), map(scripts(get(f.children, 'nodes'), 'default'), '$script', get('$script', 'content')), 'script'),
		region('template', scope('instance'), [f.fragment]),
	]);

const element = (type: string, childRule = 'NormalChildren') =>
	rule(type, { name: 'null', attributes: 'null', fragment: 'null', expression: 'omit' }, ['definition', 'properties'])
		.form((f) => seq(
			emit(f.name, get('event', 'name')), read({ kind: 'html-attributes', mode: 'normal' }, f.properties),
			emit(f.definition, choose(isType(get('record'), 'SvelteComponent'),
				at(filter(f.properties, '$attribute', and(isType(get('$attribute'), 'Attribute'),
					equal(get('$attribute', 'name'), constant('this')))), 0), at(array(), 0))),
			emit(f.expression, choose(isType(get('record'), 'SvelteComponent'),
				attributeExpression(f.definition), at(array(), 0))),
			emit(f.attributes, choose(present(f.expression),
				filter(f.properties, '$attribute', not(equal(get('$attribute'), f.definition))), f.properties)),
			call(childRule, f.fragment),
		))
		.regions((f) => {
			const self = get('record');
			const ordinary = not(component(self));
			const lets = filter(f.attributes, '$prop', isType(get('$prop'), 'LetDirective'));
			const localAttributes = choose(isType(self, 'SvelteElement'),
				filter(f.attributes, '$prop', not(and(isType(get('$prop'), 'Attribute'),
					equal(get('$prop', 'name'), constant('this'))))), f.attributes);
			const nodes = get(f.fragment, 'nodes');
			return [
				region('local', incoming, concat(localAttributes, array(f.fragment)), 'block', ordinary),
				region('contents', scope('local'), [f.fragment], 'fragment', ordinary),
				region('defaultSlot', incoming, concat(map(lets, '$let', get('$let', 'expression')),
					filter(nodes, '$child', not(named(get('$child'))))), 'fragment', and(component(self), not(named(self)))),
				{ ...region('namedChild', incoming, array(get('$child')), 'fragment', component(self)),
					each: { list: filter(nodes, '$candidateChild', named(get('$candidateChild'))), as: '$child' } },
			];
		});
rules.Element = element('Element');
rules.RawElement = element('Element', 'RawChildren');
rules.RcdataElement = element('Element', 'RcdataChildren');
rules.Script = rule('Script', { context: 'null', content: 'null', attributes: 'null' }).form((f) => seq(
	read({ kind: 'html-attributes', mode: 'static' }, f.attributes),
	js('program', f.content, get('event', 'rawChildren')),
	emit(f.context, choose(or(hasAttribute(event, 'module'), equal(staticAttribute(event, 'context'), constant('module'))), constant('module'), constant('default'))),
));
rules.Style = rule('StyleSheet', { attributes: 'null', children: 'null', comments: 'null', content: 'null' }, ['sheet']).form((f) => seq(
	read({ kind: 'html-attributes', mode: 'static' }, f.attributes),
	read({ kind: 'css-stylesheet' }, f.sheet, get('event', 'rawChildren')),
	emit(f.children, get(f.sheet, 'children')), emit(f.comments, get(f.sheet, 'comments')),
	emit(f.content, record(null, { styles: get('event', 'rawChildren', 'text'), comment: constant(null) }, get('event', 'rawChildren', 'span'))),
));

const directiveName = (f: { name: Slot<'name'>; modifiers: Slot<'modifiers'> }) =>
	seq(emit(f.name, get('event', 'argument', 'text')), emit(f.modifiers, get('event', 'modifiers')));
const directiveValue = <E extends JS, B extends JS>(into: Slot<'expression'>, entry: E, fallback?: B) => choice(
	seq(test(present(get('event', 'value'))), read({ kind: 'html-single', entry }, into, get('event', 'value'))),
	seq(test(not(present(get('event', 'value')))), fallback ? js(fallback, into, get('event', 'argument')) : seq()),
);
const directive = <E extends JS, B extends JS>(type: string, entry: E, fallback?: B) =>
	rule(type, { name: 'null', modifiers: 'null', expression: 'null' }).form((f) => seq(directiveName(f), directiveValue(f.expression, entry, fallback)));
rules.Bind = directive('BindDirective', 'expression', 'identifierReference');
rules.Class = directive('ClassDirective', 'expression', 'identifierReference');
rules.On = directive('OnDirective', 'expression');
rules.Use = directive('UseDirective', 'expression');
rules.Animate = directive('AnimateDirective', 'expression');
rules.StyleDirective = rule('StyleDirective', { name: 'null', modifiers: 'null', value: 'null' })
	.form((f) => seq(directiveName(f), read({ kind: 'html-attribute-parts' }, f.value)));
for (const [name, intro, outro] of [['Transition', true, true], ['In', true, false], ['Out', false, true]] as const) {
	rules[name] = rule('TransitionDirective', { name: 'null', modifiers: 'null', expression: 'null', intro: 'null', outro: 'null' })
		.form((f) => seq(directiveName(f), directiveValue(f.expression, 'expression'), emit(f.intro, constant(intro)), emit(f.outro, constant(outro))));
}
rules.Let = directive('LetDirective', 'pattern', 'bindingIdentifier').declares((f) => [declare([f.expression],
	choose(component(get('owner')), choose(named(get('owner')), incoming, get('owner', 'scopes', 'defaultSlot')), get('owner', 'scopes', 'local')))]);

rules.Expression = rule('ExpressionTag', { expression: 'null' }).form((f) => header(js('expression', f.expression)));
rules.Unmarked = rule('UnmarkedTag', { expression: 'omit', declaration: 'omit' })
	.form((f) => choice(
		seq(test(member(get('event', 'name'), ['let', 'const', 'type'])),
			header(js('statement', f.declaration), test(and(isType(f.declaration, 'VariableDeclaration'),
				member(get(f.declaration, 'kind'), ['let', 'const']))))),
		header(js('expression', f.expression)),
	));
rules.Spread = rule('SpreadAttribute', { expression: 'null' }).form((f) => header(token('...', true), js('expression', f.expression)));
for (const [name, prefix, type] of [['Attach', '@attach', 'AttachTag'], ['Html', '@html', 'HtmlTag'], ['Render', '@render', 'RenderTag']] as const) {
	rules[name] = rule(type, { expression: 'null' }).form((f) => header(token(prefix), space(), js('expression', f.expression)));
}
rules.Shorthand = rule('Attribute', { name: 'null', value: 'null' }, ['id']).form((f) => seq(
	header(js('identifierReference', f.id)),
	emit(f.name, get(f.id, 'name')),
	emit(f.value, record('ExpressionTag', { expression: f.id }, get(f.id, 'span'))),
));
rules.Debug = rule('DebugTag', { identifiers: 'null' }, ['first', 'rest']).form((f) => header(token('@debug'), choice(
	seq(js('identifierReference', f.first), repeat((r) => seq(token(','), js('identifierReference', r.item)), f.rest),
		emit(f.identifiers, concat(array(f.first), f.rest))),
	emit(f.identifiers, array()),
)));
rules.Declarator = rule('js.VariableDeclarator', { id: 'null', init: 'null' })
	.form((f) => seq(js('pattern', f.id), token('='), js('assignmentExpression', f.init)));
rules.ConstDeclaration = rule('js.VariableDeclaration', { kind: 'null', declarations: 'null' })
	.form((f) => seq(token('const', true), space(), many('Declarator', f.declarations, 1, 1), emit(f.kind, constant('const'))))
	.span('through-next-token-start');
rules.Const = rule('ConstTag', { declaration: 'null' }).form((f) => header(token('@'), call('ConstDeclaration', f.declaration)));

const ifRule = (continuation: boolean) => rule('IfBlock', { test: 'null', consequent: 'null', alternate: 'null', elseif: 'null' })
	.form((f) => seq(
		token('{'), continuation ? seq(token(':else'), space(), token('if')) : token('#if'),
		space(), js('expression', f.test), token('}'), body(f.consequent),
		choice(call('ElseIf', f.alternate), seq(branch(token('else', true)), body(f.alternate), end('if')), end('if')),
		emit(f.elseif, constant(continuation)),
	))
	.regions((f) => [region('yes', incoming, [f.consequent]), region('no', incoming, [f.alternate])]);
rules.If = ifRule(false);
rules.IfContinuation = ifRule(true);
rules.ElseIf = rule('Fragment', { nodes: 'null' }).form((f) => many('IfContinuation', f.nodes, 1, 1)).span('none');
rules.Each = rule('EachBlock', { expression: 'null', context: 'null', body: 'null', index: 'omit', key: 'omit', fallback: 'omit' })
	.form((f) => seq(
		header(token('#each'), space(), js('expression', f.expression, undefined, 'last-shared-word'),
			optional(seq(token('as'), space(), js('pattern', f.context))),
			optional(seq(token(','), js('bindingIdentifier', f.index))),
			optional(seq(token('('), js('expression', f.key), token(')')))),
		body(f.body), optional(seq(branch(token('else', true)), body(f.fallback))), end('each'),
	))
	.regions((f) => [region('iteration', incoming, [f.context, f.index, f.key, f.body]), region('empty', incoming, [f.fallback])])
	.declares((f) => [declare([f.context, f.index], 'iteration')]);

rules.Await = rule('AwaitBlock', { expression: 'null', value: 'null', error: 'null', pending: 'null', then: 'null', catch: 'null' })
	.form((f) => {
		const thenHeader = (tight = false) => seq(token('then', tight), optional(seq(space(), js('pattern', f.value))));
		const catchHeader = (tight = false) => seq(token('catch', tight), optional(seq(space(), js('pattern', f.error))));
		const thenBody = () => seq(branch(thenHeader(true)), body(f.then));
		const catchBody = () => seq(branch(catchHeader(true)), body(f.catch));
		return seq(
			token('{'), token('#await'), space(), js('expression', f.expression),
			choice(
				seq(thenHeader(), token('}'), body(f.then), optional(catchBody())),
				seq(catchHeader(), token('}'), body(f.catch), optional(thenBody())),
				seq(token('}'), body(f.pending), choice(
					seq(thenBody(), optional(catchBody())),
					seq(catchBody(), optional(thenBody())),
					seq(),
				)),
			),
			end('await'),
		);
	})
	.regions((f) => [
		region('waiting', incoming, [f.pending]),
		region('resolved', incoming, [f.value, f.then]),
		region('rejected', incoming, [f.error, f.catch]),
	])
	.declares((f) => [declare([f.value], 'resolved'), declare([f.error], 'rejected')]);
rules.Key = rule('KeyBlock', { expression: 'null', fragment: 'null' })
	.form((f) => seq(header(token('#key'), space(), js('expression', f.expression)), body(f.fragment), end('key')))
	.regions((f) => [region('content', incoming, [f.fragment])]);
rules.Snippet = rule('SnippetBlock', { expression: 'null', parameters: 'null', body: 'null', typeParams: 'omit' }, ['types'])
	.form((f) => seq(
		header(token('#snippet'), space(), js('bindingIdentifier', f.expression),
			optional(js('typeParameters', f.types)), js('params', f.parameters)),
		body(f.body), end('snippet'),
		emit(f.typeParams, get(f.types, 'innerSource')),
	))
	.regions((f) => [region('function', incoming, [f.types, f.parameters, f.body], 'function')])
	.declares((f) => [declare([f.expression], 'incoming'), declare([f.parameters], 'function', 'param')]);

const nameIs = (name: string) => equal(get('event', 'name'), constant(name));
const nearestHeadBoundary = at(filter(get('ancestors'), '$ancestor', or(equal(get('$ancestor', 'name'), constant('svelte:head')),
	isType(get('$ancestor'), 'RegularElement', 'Component'))), 0);
const elements: Dispatch[] = [
	{ when: and(nameIs('script'), get('event', 'atDocument')), rule: 'Script', attributes: 'static', content: 'raw' },
	{ when: and(nameIs('style'), get('event', 'atDocument')), rule: 'Style', attributes: 'static', content: 'raw' },
];
for (const [name, type] of Object.entries({ element: 'SvelteElement', component: 'SvelteComponent', self: 'SvelteSelf',
	window: 'SvelteWindow', document: 'SvelteDocument', body: 'SvelteBody', head: 'SvelteHead', options: 'SvelteOptions',
	fragment: 'SvelteFragment', boundary: 'SvelteBoundary' })) elements.push({ when: nameIs('svelte:' + name), rule: 'Element', type });
elements.push(
	{ when: and(nameIs('title'), equal(get(nearestHeadBoundary, 'name'), constant('svelte:head'))), rule: 'Element', type: 'TitleElement' },
	{ when: and(nameIs('slot'), not(any(filter(get('ancestors'), '$ancestor', and(isType(get('$ancestor'), 'RegularElement'),
		hasAttribute(get('$ancestor'), 'shadowrootmode')))))), rule: 'Element', type: 'SlotElement' },
	{ when: nameIs('textarea'), rule: 'RcdataElement', type: 'RegularElement', content: 'rcdata' },
	{ when: member(get('event', 'name'), ['script', 'style']), rule: 'RawElement', type: 'RegularElement', content: 'raw' },
	{ when: or(and(get('event', 'nameFacts', 'uppercaseInitial'), get('event', 'nameFacts', 'identifier')), get('event', 'nameFacts', 'dottedIdentifier')), rule: 'Element', type: 'Component' },
	{ when: and(get('event', 'nameFacts', 'validHtmlName'), not(equal(get('event', 'nameFacts', 'namespace'), constant('svelte')))), rule: 'Element', type: 'RegularElement' },
);
export const svelte = {
	version: 1, document: 'Document', rules,
	html: {
		delimiters: ['{', '}'], attributeInterpolations: true, attributeComments: 'javascript', autoclose: true, typescript: [{ element: 'script', attribute: 'lang', value: 'ts' }],
		trimEnd: true,
		void: ['area', 'base', 'br', 'col', 'command', 'embed', 'hr', 'img', 'input', 'keygen', 'link', 'meta', 'param', 'source', 'track', 'wbr'],
		text: 'Text', comment: 'Comment', plainAttribute: { type: 'Attribute', name: 'name', value: 'value', text: 'Text', expression: 'Expression' },
		content: [
			{ prefix: '{#if', rule: 'If' }, { prefix: '{#each', rule: 'Each' }, { prefix: '{#await', rule: 'Await' },
			{ prefix: '{#key', rule: 'Key' }, { prefix: '{#snippet', rule: 'Snippet' }, { prefix: '{@html', rule: 'Html' },
			{ prefix: '{@debug', rule: 'Debug' }, { prefix: '{@const', rule: 'Const' }, { prefix: '{@render', rule: 'Render' },
			{ prefix: '{', rule: 'Unmarked' },
		],
		attribute: [{ prefix: '{...', rule: 'Spread' }, { prefix: '{@attach', rule: 'Attach' }, { prefix: '{', rule: 'Shorthand' }],
		elements,
		directiveNames: { prefix: '', argument: ':', modifier: '|', requireArgument: true, dynamic: null, unknown: 'plain-attribute' },
		directives: Object.entries({ bind: 'Bind', on: 'On', use: 'Use', class: 'Class', style: 'StyleDirective', transition: 'Transition',
			in: 'In', out: 'Out', animate: 'Animate', let: 'Let' }).map(([name, rule]) => ({ name, rule })),
	},
} satisfies Plan;
