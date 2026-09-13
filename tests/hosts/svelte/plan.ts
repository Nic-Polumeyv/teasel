import {
  type V, type F, type JS, type Stop, type Mode, type Rule, type Dispatch, type Plan, constant, get,
  equal, present, choose, at, array, record, not, and, or, filter, map, any, member, concat, field, scope,
  incoming, isType, hasAttribute, staticAttribute, seq, choice, optional, read, emit, token, space, test,
  call, js, fields, rule, region, declare, repeat, many,
} from '../../../package/plan';

const named = (node: V) => present(staticAttribute(node, 'slot'));
const component = (node: V) => isType(node, 'Component', 'SvelteComponent', 'SvelteSelf');
const header = (...items: F[]) => seq(token('{'), ...items, token('}'));
const end = (name: string) => header(token('/' + name, true));
const branch = (...items: F[]) => header(token(':', true), ...items);
const body = (into: string) => call('Body', into);

const rules: Record<string, Rule> = {};
rules.Text = rule('Text', ['data', 'raw'], seq(emit('data', get('event', 'decoded')), emit('raw', get('event', 'raw'))));
rules.Comment = rule('Comment', ['data'], emit('data', get('event', 'data')));
const children = (mode: Mode, stop: Stop) => rule('Fragment', ['nodes'], read({ kind: 'html-children', mode, stop }, 'nodes'), { span: 'none' });
rules.Body = children('normal', { prefixes: ['{:', '{/'] });
rules.DocumentChildren = children('normal', { documentEnd: true });
rules.NormalChildren = children('normal', { matchingElement: true });
rules.RawChildren = children('raw', { matchingElement: true });
rules.RcdataChildren = children('rcdata', { matchingElement: true });
const rootNodes = get(field('fragment'), 'nodes');
const scripts = (context: string) => filter(rootNodes, '$node',
  and(isType(get('$node'), 'Script'), equal(get('$node', 'context'), constant(context))));
rules.Document = rule('Root', ['fragment'], call('DocumentChildren', 'fragment'), {
  regions: [
    region('module', constant(null), map(scripts('module'), '$script', get('$script', 'content')), 'module'),
    region('instance', scope('module'), map(scripts('default'), '$script', get('$script', 'content')), 'script'),
    region('template', scope('instance'), filter(rootNodes, '$node', not(isType(get('$node'), 'Script', 'StyleSheet'))))
  ]
});

const element = (type: string, childRule = 'NormalChildren'): Rule => {
  const self = get('record');
  const ordinary = not(component(self));
  const lets = filter(field('attributes'), '$prop', isType(get('$prop'), 'LetDirective'));
  const localAttributes = filter(field('attributes'), '$prop', not(and(isType(self, 'SvelteElement'),
    and(isType(get('$prop'), 'Attribute'), equal(get('$prop', 'name'), constant('this'))))));
  const nodes = get(field('fragment'), 'nodes');
  return rule(type, ['name', 'attributes', 'fragment'], seq(
    emit('name', get('event', 'name')), read({ kind: 'html-attributes', mode: 'normal' }, 'attributes'), call(childRule, 'fragment')
  ), { regions: [
    region('local', incoming, concat(localAttributes, array(field('fragment'))), 'block', ordinary),
    region('contents', scope('local'), array(field('fragment')), 'fragment', ordinary),
    region('defaultSlot', incoming, concat(map(lets, '$let', get('$let', 'expression')),
      filter(nodes, '$child', not(named(get('$child'))))), 'fragment', and(component(self), not(named(self)))),
    { ...region('namedChild', incoming, array(get('$child')), 'fragment', component(self)),
      each: { list: filter(nodes, '$candidateChild', named(get('$candidateChild'))), as: '$child' } }
  ] });
};
rules.Element = element('Element');
rules.RawElement = element('Element', 'RawChildren');
rules.RcdataElement = element('Element', 'RcdataChildren');
const event = get('event');
rules.Script = rule('Script', ['context', 'content', 'attributes'], seq(
  read({ kind: 'html-attributes', mode: 'static' }, 'attributes'),
  js('program', 'content', get('event', 'rawChildren')),
  emit('context', choose(or(hasAttribute(event, 'module'), equal(staticAttribute(event, 'context'), constant('module'))), constant('module'), constant('default')))
));
rules.Style = rule('StyleSheet', ['attributes', 'children', 'comments', 'content'], seq(
  read({ kind: 'html-attributes', mode: 'static' }, 'attributes'),
  read({ kind: 'css-stylesheet' }, 'sheet', get('event', 'rawChildren')),
  emit('children', get('locals', 'sheet', 'children')), emit('comments', get('locals', 'sheet', 'comments')),
  emit('content', record(null, { styles: get('event', 'rawChildren', 'text'), comment: constant(null) }, get('event', 'rawChildren', 'span')))
), { locals: ['sheet'] });

const directiveName = () => seq(emit('name', get('event', 'argument', 'text')), emit('modifiers', get('event', 'modifiers')));
const directiveValue = (entry: JS, fallback?: JS) => choice(
  seq(test(present(get('event', 'value'))), read({ kind: 'html-single', entry }, 'expression', get('event', 'value'))),
  seq(test(not(present(get('event', 'value')))), fallback ? js(fallback, 'expression', get('event', 'argument')) : seq())
);
const directive = (type: string, entry: JS = 'expression', fallback?: JS) =>
  rule(type, ['name', 'modifiers', 'expression'], seq(directiveName(), directiveValue(entry, fallback)));
rules.Bind = directive('BindDirective', 'expression', 'identifierReference');
rules.Class = directive('ClassDirective', 'expression', 'identifierReference');
rules.On = directive('OnDirective');
rules.Use = directive('UseDirective');
rules.Animate = directive('AnimateDirective');
rules.StyleDirective = rule('StyleDirective', ['name', 'modifiers', 'value'], seq(directiveName(), read({ kind: 'html-attribute-parts' }, 'value')));
for (const [name, intro, outro] of [['Transition', true, true], ['In', true, false], ['Out', false, true]] as const) {
  rules[name] = rule('TransitionDirective', ['name', 'modifiers', 'expression', 'intro', 'outro'],
    seq(directiveName(), directiveValue('expression'), emit('intro', constant(intro)), emit('outro', constant(outro))));
}
rules.Let = { ...directive('LetDirective', 'pattern', 'bindingIdentifier'), declares: [declare(array(field('expression')),
  choose(component(get('owner')), choose(named(get('owner')), incoming, get('owner', 'scopes', 'defaultSlot')), get('owner', 'scopes', 'local')))] };

rules.Expression = rule('ExpressionTag', ['expression'], header(js('expression', 'expression')));
rules.Unmarked = rule('UnmarkedTag', [], choice(header(js('expression', 'expression')), header(js('statement', 'declaration'))),
  { fields: fields([], ['expression', 'declaration']) });
rules.Spread = rule('SpreadAttribute', ['expression'], header(token('...', true), js('expression', 'expression')));
for (const [name, prefix, type] of [['Attach', '@attach', 'AttachTag'], ['Html', '@html', 'HtmlTag'], ['Render', '@render', 'RenderTag']] as const) {
  rules[name] = rule(type, ['expression'], header(token(prefix, true), space(), js('expression', 'expression')));
}
rules.Shorthand = rule('Attribute', ['name', 'value'], seq(header(js('identifierReference', 'id')),
  emit('name', get('locals', 'id', 'name')),
  emit('value', record('ExpressionTag', { expression: get('locals', 'id') }, get('locals', 'id', 'span')))
), { locals: ['id'] });
rules.Debug = rule('DebugTag', ['identifiers'], header(token('@debug', true), choice(
  seq(js('identifierReference', 'first'), repeat(seq(token(','), js('identifierReference', 'item')), 'rest', get('iteration', 'item')),
    emit('identifiers', concat(array(get('locals', 'first')), get('locals', 'rest')))),
  emit('identifiers', array())
)), { locals: ['first', 'rest'] });
rules.Declarator = rule('js.VariableDeclarator', ['id', 'init'], seq(js('pattern', 'id'), token('='), js('assignmentExpression', 'init')));
rules.ConstDeclaration = rule('js.VariableDeclaration', ['kind', 'declarations'], seq(token('const', true), space(),
  many('Declarator', 'declarations', 1, 1), emit('kind', constant('const'))), { span: 'through-next-token-start' });
rules.Const = rule('ConstTag', ['declaration'], header(token('@', true), call('ConstDeclaration', 'declaration')));

const ifRule = (continuation: boolean) => rule('IfBlock', ['test', 'consequent', 'alternate', 'elseif'], seq(
  token('{'), continuation ? seq(token(':else', true), space(), token('if')) : token('#if', true),
  space(), js('expression', 'test'), token('}'), body('consequent'),
  choice(call('ElseIf', 'alternate'), seq(branch(token('else', true)), body('alternate'), end('if')), end('if')),
  emit('elseif', constant(continuation))
), { regions: [region('yes', incoming, array(field('consequent'))), region('no', incoming, array(field('alternate')))] });
rules.If = ifRule(false);
rules.IfContinuation = ifRule(true);
rules.ElseIf = rule('Fragment', ['nodes'], many('IfContinuation', 'nodes', 1, 1), { span: 'none' });
rules.Each = rule('EachBlock', [], seq(
  header(token('#each', true), space(), js('expression', 'expression', undefined, 'last-shared-word'),
    optional(seq(token('as'), space(), js('pattern', 'context'))),
    optional(seq(token(','), js('bindingIdentifier', 'index'))),
    optional(seq(token('('), js('expression', 'key'), token(')')))),
  body('body'), optional(seq(branch(token('else', true)), body('fallback'))), end('each')
), { fields: fields(['expression', 'context', 'body'], ['index', 'key', 'fallback']),
  regions: [region('iteration', incoming, array(field('context'), field('index'), field('key'), field('body'))), region('empty', incoming, array(field('fallback')))],
  declares: [declare(array(field('context'), field('index')), scope('iteration'))] });

const thenHeader = (tight = false) => seq(token('then', tight), optional(seq(space(), js('pattern', 'value'))));
const catchHeader = (tight = false) => seq(token('catch', tight), optional(seq(space(), js('pattern', 'error'))));
const thenBody = () => seq(branch(thenHeader(true)), body('then'));
const catchBody = () => seq(branch(catchHeader(true)), body('catch'));
rules.Await = {
  type: 'AwaitBlock',
  fields: { expression: 'null', value: 'null', error: 'null', pending: 'null', then: 'null', catch: 'null' },
  form: {
    op: 'seq', items: [
      token('{'), token('#await', true), space(), js('expression', 'expression'),
      { op: 'choice', alternatives: [
        { op: 'seq', items: [thenHeader(), token('}'), body('then'), optional(catchBody())] },
        { op: 'seq', items: [catchHeader(), token('}'), body('catch'), optional(thenBody())] },
        { op: 'seq', items: [token('}'), body('pending'),
          { op: 'choice', alternatives: [
            { op: 'seq', items: [thenBody(), optional(catchBody())] },
            { op: 'seq', items: [catchBody(), optional(thenBody())] },
            { op: 'seq', items: [] }
          ] }
        ] }
      ] },
      end('await')
    ]
  },
  regions: [
    region('waiting', incoming, array(field('pending'))),
    region('resolved', incoming, array(field('value'), field('then'))),
    region('rejected', incoming, array(field('error'), field('catch')))
  ],
  declares: [declare(array(field('value')), scope('resolved')), declare(array(field('error')), scope('rejected'))]
};
rules.Key = rule('KeyBlock', ['expression', 'fragment'], seq(header(token('#key', true), space(), js('expression', 'expression')),
  body('fragment'), end('key')), { regions: [region('content', incoming, array(field('fragment')))] });
rules.Snippet = rule('SnippetBlock', [], seq(header(token('#snippet', true), space(), js('bindingIdentifier', 'expression'),
  optional(js('typeParameters', 'types')), js('params', 'parameters')), body('body'), end('snippet'),
  emit('typeParams', get('locals', 'types', 'innerSource'))
), { fields: fields(['expression', 'parameters', 'body'], ['typeParams']), locals: ['types'],
  regions: [region('function', incoming, array(get('locals', 'types'), field('parameters'), field('body')), 'function')],
  declares: [declare(array(field('expression')), incoming), declare(array(field('parameters')), scope('function'), 'param')] });

const nameIs = (name: string) => equal(get('event', 'name'), constant(name));
const nearestHeadBoundary = at(filter(get('ancestors'), '$ancestor', or(equal(get('$ancestor', 'name'), constant('svelte:head')),
  isType(get('$ancestor'), 'RegularElement', 'Component'))), 0);
const elements: Dispatch[] = [
  { when: and(nameIs('script'), get('event', 'atDocument')), rule: 'Script', attributes: 'static', content: 'raw' },
  { when: and(nameIs('style'), get('event', 'atDocument')), rule: 'Style', attributes: 'static', content: 'raw' }
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
  { when: get('event', 'nameFacts', 'validHtmlName'), rule: 'Element', type: 'RegularElement' }
);
export const svelte: Plan = {
  version: 1, document: 'Document', rules,
  html: {
    delimiters: ['{', '}'], attributeInterpolations: true, attributeComments: 'javascript', autoclose: true, trimEnd: true,
    void: ['area', 'base', 'br', 'col', 'command', 'embed', 'hr', 'img', 'input', 'keygen', 'link', 'meta', 'param', 'source', 'track', 'wbr'],
    text: 'Text', comment: 'Comment', plainAttribute: { type: 'Attribute', name: 'name', value: 'value', text: 'Text', expression: 'Expression' },
    content: [
      { prefix: '{#if', rule: 'If' }, { prefix: '{#each', rule: 'Each' }, { prefix: '{#await', rule: 'Await' },
      { prefix: '{#key', rule: 'Key' }, { prefix: '{#snippet', rule: 'Snippet' }, { prefix: '{@html', rule: 'Html' },
      { prefix: '{@debug', rule: 'Debug' }, { prefix: '{@const', rule: 'Const' }, { prefix: '{@render', rule: 'Render' },
      { prefix: '{', rule: 'Unmarked' }
    ],
    attribute: [{ prefix: '{...', rule: 'Spread' }, { prefix: '{@attach', rule: 'Attach' }, { prefix: '{', rule: 'Shorthand' }],
    elements,
    directiveNames: { prefix: '', argument: ':', modifier: '|', requireArgument: true, dynamic: null, unknown: 'plain-attribute' },
    directives: Object.entries({ bind: 'Bind', on: 'On', use: 'Use', class: 'Class', style: 'StyleDirective', transition: 'Transition',
      in: 'In', out: 'Out', animate: 'Animate', let: 'Let' }).map(([name, rule]) => ({ name, rule }))
  }
};
