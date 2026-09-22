// written by crates/teasel/src/host/plan.rs

export type Json = null | boolean | number | string | ReadonlyArray<Json> | { readonly [key: string]: Json };
export type Absence = 'null' | 'omit';
export type Gap = 'space*' | 'none';
export type Mode = 'normal' | 'raw' | 'rcdata' | 'verbatim';
export type AttributeMode = 'normal' | 'static';
export type AttributeComments = 'javascript' | 'none';
export type UnknownDirective = 'plain-attribute' | 'wildcard-rule';
export type SpanPolicy = 'none' | 'through-next-token-start';
export type Boundary = 'last-shared-word';
export type RegionKind = 'module' | 'script' | 'fragment' | 'block' | 'function';
export type DeclareKind = 'pattern' | 'param';
export type Relation = 'equal' | 'less' | 'present';
export type Js = 'expression' | 'assignmentExpression' | 'pattern' | 'bindingIdentifier' | 'identifierReference' | 'params' | 'typeParameters' | 'statement' | 'program';
export type Plan = { 'version': 1; 'document': string; 'rules': { readonly [key: string]: Rule }; 'html': Html; };
export type Html = { 'delimiters': readonly [string, string]; 'attributeInterpolations': boolean; 'attributeComments': AttributeComments; 'autoclose': boolean; 'trimEnd': boolean; 'void': ReadonlyArray<string>; 'text': string; 'comment': string; 'content': ReadonlyArray<PrefixDispatch>; 'attribute': ReadonlyArray<PrefixDispatch>; 'plainAttribute': PlainAttribute; 'elements': ReadonlyArray<Dispatch>; 'directiveNames': DirectiveNames; 'directives': ReadonlyArray<NamedDispatch>; };
export type Rule = { 'type': string; 'fields': { readonly [key: string]: Absence }; 'locals'?: ReadonlyArray<string>; 'form': Form; 'regions'?: ReadonlyArray<Region>; 'declares'?: ReadonlyArray<Declare>; 'span'?: SpanPolicy; };
export type Form =
	| ({ 'op': 'seq'; } & { 'items': ReadonlyArray<Form>; })
	| ({ 'op': 'choice'; } & { 'alternatives': ReadonlyArray<Form>; })
	| ({ 'op': 'repeat'; } & { 'body': Form; 'min': number; 'max': number | null; 'locals': ReadonlyArray<string>; 'yield': Value; 'into': string; })
	| ({ 'op': 'read'; } & { 'reader': Reader; 'into'?: string; 'input'?: Value; })
	| ({ 'op': 'emit'; } & { 'into': string; 'value': Value; });
export type Reader =
	| ({ 'kind': 'token'; } & { 'text': string; 'gap': Gap; 'word': boolean; })
	| ({ 'kind': 'space'; } & { 'min': number; })
	| ({ 'kind': 'test'; } & { 'value': Value; })
	| ({ 'kind': 'rule'; } & { 'name': string; })
	| ({ 'kind': 'javascript'; } & { 'entry': Js; 'boundary'?: Boundary; })
	| ({ 'kind': 'html-single'; } & { 'entry': Js; })
	| ({ 'kind': 'html-attributes'; } & { 'mode': AttributeMode; })
	| ({ 'kind': 'html-attribute-parts'; } & { })
	| ({ 'kind': 'html-children'; } & { 'mode': Mode; 'stop': Stop; })
	| ({ 'kind': 'css-stylesheet'; } & { });
export type Value =
	| ({ 'op': 'constant'; } & { 'value': Json; })
	| ({ 'op': 'get'; } & { 'base': string | Value; 'path': ReadonlyArray<string | number>; })
	| ({ 'op': 'compare'; } & Compare)
	| ({ 'op': 'choose'; } & { 'condition': Value; 'yes': Value; 'no': Value; })
	| ({ 'op': 'flatMap'; } & { 'list': Value; 'as': string; 'body': Value; })
	| ({ 'op': 'length'; } & { 'list': Value; })
	| ({ 'op': 'at'; } & { 'list': Value; 'index': Value; })
	| ({ 'op': 'construct'; } & Construct);
export type Compare =
	| ({ 'relation': 'present'; } & { 'left': Value; })
	| ({ 'relation': 'equal' | 'less'; } & { 'left': Value; 'right': Value; });
export type Construct =
	| ({ 'shape': 'array'; } & { 'items': ReadonlyArray<Value>; })
	| ({ 'shape': 'record'; } & { 'type': string | null; 'fields': { readonly [key: string]: Value }; 'span': Value; });
export type Region = { 'id': string; 'parent': Value; 'kind': RegionKind; 'covers': Value; 'when'?: Value; 'each'?: Each; };
export type Each = { 'list': Value; 'as': string; };
export type Declare = { 'patterns': Value; 'into': Value; 'kind': DeclareKind; };
export type Stop = { 'prefixes': ReadonlyArray<string>; 'matchingElement'?: never; 'documentEnd'?: never; } | { 'matchingElement': true; 'prefixes'?: never; 'documentEnd'?: never; } | { 'documentEnd': true; 'prefixes'?: never; 'matchingElement'?: never; };
export type PrefixDispatch = { 'prefix': string; 'rule': string; };
export type NamedDispatch = { 'name': string; 'rule': string; };
export type Dispatch = { 'when': Value; 'rule': string; 'type'?: string; 'attributes'?: 'static'; 'content'?: Mode; };
export type PlainAttribute = { 'type': string; 'name': string; 'value': string; 'text': string; 'expression': string; };
export type DirectiveNames = { 'prefix': string; 'argument': string; 'modifier': string; 'requireArgument': boolean; 'dynamic': readonly [string, string] | null; 'unknown': UnknownDirective; };
