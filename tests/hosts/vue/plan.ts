import {
  type Plan, type Rule, type Form, type Mode, type Region, type Dispatch,
  rule, seq, choice, optional, read, emit, token, js, get, constant, equal,
  field, scope, incoming, region, declare, array, concat, filter, map,
  and, or, not, any, isType, member, hasAttribute, present, test,
} from '../../../package/plan';

const rules: Record<string, Rule> = {};
rules.Text = rule('Text', ['content'], emit('content', get('event', 'decoded')));
rules.Comment = rule('Comment', ['content'], emit('content', get('event', 'data')));
rules.Document = rule('Root', ['children'], read({
  kind: 'html-children', mode: 'normal', stop: { documentEnd: true },
}, 'children'), {
  regions: [region('template', constant(null), array(field('children')), 'module')],
});
rules.Interpolation = rule('Interpolation', ['content'], seq(
  token('{{'), js('expression', 'content'), token('}}'),
));

const vueDirective = (name: string) => filter(field('props'), '$directive',
  and(isType(get('$directive'), 'Directive'), equal(get('$directive', 'name'), constant(name))));
const templateSlot = and(isType(get('record'), 'Template'), any(vueDirective('slot')));
const regions: Region[] = [
  region('loop', incoming, concat(
    filter(field('props'), '$prop', not(and(isType(get('$prop'), 'Directive'),
      or(equal(get('$prop', 'name'), constant('for')), and(not(templateSlot), member(get('$prop', 'name'), ['if', 'else-if'])))))),
    array(field('children'))
  ), 'block', any(vueDirective('for'))),
  region('slot', scope('loop'), concat(map(vueDirective('slot'), '$slot', get('$slot', 'props')), array(field('children'))),
    'function', any(vueDirective('slot')))
];
const element = (type: string, mode: Mode = 'normal', staticAttributes = false) => rule(type, ['tag', 'props', 'children'], seq(
  emit('tag', get('event', 'name')),
  read({ kind: 'html-attributes', mode: staticAttributes ? 'static' : 'normal' }, 'props'),
  read({ kind: 'html-children', mode, stop: { matchingElement: true } }, 'children'),
), { regions });
rules.Element = element('Element');
rules.VerbatimElement = element('Element', 'verbatim', true);
rules.RawElement = element('Element', 'raw');
rules.RcdataElement = element('Element', 'rcdata');

const directiveName = (name?: string, prop = false): Form => seq(
  emit('name', name ? constant(name) : get('event', 'name')),
  emit('rawName', get('event', 'rawName')),
  emit('modifiers', prop ? concat(get('event', 'modifiers'), array(constant('prop'))) : get('event', 'modifiers')),
  choice(
    seq(test(and(present(get('event', 'argument')), get('event', 'argument', 'dynamic'))),
      js('expression', 'arg', get('event', 'argument'))),
    seq(test(not(and(present(get('event', 'argument')), get('event', 'argument', 'dynamic')))),
      emit('arg', get('event', 'argument', 'text'))),
  ),
);
const optionalValue = (form: Form): Form => choice(
  seq(read({ kind: 'test', value: present(get('event', 'value')) }), form),
  read({ kind: 'test', value: not(present(get('event', 'value'))) }),
);
const directiveFields = ['name', 'arg', 'modifiers', 'rawName'];
const aliases = () => seq(
  optional(js('pattern', 'value')),
  optional(seq(token(','), optional(js('pattern', 'key')),
    optional(seq(token(','), optional(js('pattern', 'index')))))),
);
rules.ForValue = rule('ForAliases', ['value', 'key', 'index', 'source'], seq(
  choice(seq(token('('), aliases(), token(')')), aliases()),
  choice(token('in'), token('of')), js('expression', 'source'),
));
rules.For = rule('Directive', [...directiveFields, 'value', 'key', 'index', 'source'], seq(
  directiveName('for'),
  read({ kind: 'rule', name: 'ForValue' }, 'parsed', get('event', 'value')),
  ...['value', 'key', 'index', 'source'].map(name => emit(name, get('locals', 'parsed', name))),
), { locals: ['parsed'], declares: [declare(array(field('value'), field('key'), field('index')), get('owner', 'scopes', 'loop'))] });
rules.SlotValue = rule('SlotProps', ['props'], seq(
  optional(js('pattern', 'props')), read({ kind: 'space', min: 0 }),
));
rules.ExpressionValue = rule('DirectiveExpression', ['exp'], seq(
  optional(js('expression', 'exp')), read({ kind: 'space', min: 0 }),
));
rules.SlotDirective = rule('Directive', [...directiveFields, 'props'], seq(
  directiveName('slot'), optionalValue(seq(
    read({ kind: 'rule', name: 'SlotValue' }, 'parsed', get('event', 'value')),
    emit('props', get('locals', 'parsed', 'props')),
  )),
), { locals: ['parsed'], declares: [declare(array(field('props')), get('owner', 'scopes', 'slot'), 'param')] });
rules.On = rule('Directive', [...directiveFields, 'handler'], seq(
  directiveName('on'), optionalValue(choice(
    js('expression', 'handler', get('event', 'value')),
    js('program', 'handler', get('event', 'value')),
  )),
));
for (const [key, name, prop] of [['Directive', undefined, false], ['Bind', 'bind', false], ['Prop', 'bind', true]] as const) {
  rules[key] = rule('Directive', [...directiveFields, 'exp'], seq(
    directiveName(name, prop), optionalValue(seq(
      read({ kind: 'rule', name: 'ExpressionValue' }, 'parsed', get('event', 'value')),
      emit('exp', get('locals', 'parsed', 'exp')),
    )),
  ), { locals: ['parsed'] });
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
const elements: Dispatch[] = elementKinds.map(row => ({
  when: and(hasAttribute(get('event'), 'v-pre'), row.when),
  rule: 'VerbatimElement', type: row.type, attributes: 'static', content: 'verbatim',
}));
elements.push(...elementKinds);
export const vue: Plan = {
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
};
