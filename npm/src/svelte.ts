// the tree a Svelte component answers with, as the engine writes it: crates/teasel/src/host/svelte.rs and css.rs
import type { Expression, Identifier, Pattern, Program, VariableDeclaration } from 'estree';
import type { Comment as JsComment } from './types.ts';

/** What a node of the markup has: its type and its span, UTF-16 offsets into the source. */
export interface Base {
	type: string;
	start: number;
	end: number;
}

/** The component. */
export interface Root extends Base {
	type: 'Root';
	/** The style element at the top level, its sheet read. */
	css: StyleSheet | null;
	/** Always empty. */
	js: [];
	/** Always null: `<svelte:options>` stands in the fragment. */
	options: null;
	/** The comments of the JavaScript, in source order, with or without the `comments` option. */
	comments: JsComment[];
	/** The script with `context="module"` or `module`; absent without one. */
	module?: Script;
	/** The other script; absent without one. */
	instance?: Script;
	fragment: Fragment;
}

/** A list of the markup's nodes: what an element, a block's body or the component holds. It has no span. */
export interface Fragment {
	type: 'Fragment';
	nodes: FragmentNode[];
}

/** What a fragment lists. */
export type FragmentNode = Text | Comment | ElementLike | Block | Tag;

/** Text, `raw` as written and `data` with its character references decoded. */
export interface Text extends Base {
	type: 'Text';
	raw: string;
	data: string;
}

/** An HTML comment, `data` what stands between `<!--` and `-->`. */
export interface Comment extends Base {
	type: 'Comment';
	data: string;
}

export interface Script extends Base {
	type: 'Script';
	context: 'default' | 'module';
	content: Program;
	attributes: Attribute[];
}

// ── elements

/** What every element has; `name` is the tag as written. */
interface Element extends Base {
	name: string;
	attributes: AttributeLike[];
	fragment: Fragment;
}

export interface RegularElement extends Element {
	type: 'RegularElement';
}
/** A capitalized tag, or a dotted one. */
export interface Component extends Element {
	type: 'Component';
}
/** `<title>` inside `<svelte:head>`. */
export interface TitleElement extends Element {
	type: 'TitleElement';
}
/** `<slot>` outside an element with `shadowrootmode`. */
export interface SlotElement extends Element {
	type: 'SlotElement';
}
/** `<svelte:element this={tag}>`. */
export interface SvelteElement extends Element {
	type: 'SvelteElement';
	tag: Expression;
}
/** `<svelte:component this={expression}>`. */
export interface SvelteComponent extends Element {
	type: 'SvelteComponent';
	expression: Expression;
}
export interface SvelteSelf extends Element {
	type: 'SvelteSelf';
}
export interface SvelteWindow extends Element {
	type: 'SvelteWindow';
}
export interface SvelteDocument extends Element {
	type: 'SvelteDocument';
}
export interface SvelteBody extends Element {
	type: 'SvelteBody';
}
export interface SvelteHead extends Element {
	type: 'SvelteHead';
}
export interface SvelteOptions extends Element {
	type: 'SvelteOptions';
}
export interface SvelteFragment extends Element {
	type: 'SvelteFragment';
}
export interface SvelteBoundary extends Element {
	type: 'SvelteBoundary';
}

export type ElementLike =
	| RegularElement
	| Component
	| TitleElement
	| SlotElement
	| SvelteElement
	| SvelteComponent
	| SvelteSelf
	| SvelteWindow
	| SvelteDocument
	| SvelteBody
	| SvelteHead
	| SvelteOptions
	| SvelteFragment
	| SvelteBoundary;

// ── attributes and directives

/** `true` for an attribute without a value, an expression in braces alone, or text with expressions in it. */
export type AttributeValue = true | ExpressionTag | (Text | ExpressionTag)[];

/** `name="value"`, `name={value}`, `{name}` or `name`. */
export interface Attribute extends Base {
	type: 'Attribute';
	name: string;
	value: AttributeValue;
}

/** `{...expression}` among the attributes. */
export interface SpreadAttribute extends Base {
	type: 'SpreadAttribute';
	expression: Expression;
}

/** What a directive has: the name after its `:` and the modifiers after `|`. */
interface Directive extends Base {
	name: string;
	modifiers: string[];
}

/** `bind:name`, or `bind:name={expression}`; without a value the expression is the name. */
export interface BindDirective extends Directive {
	type: 'BindDirective';
	expression: Expression;
}
export interface OnDirective extends Directive {
	type: 'OnDirective';
	expression: Expression | null;
}
export interface UseDirective extends Directive {
	type: 'UseDirective';
	expression: Expression | null;
}
/** `class:name`, or `class:name={expression}`; without a value the expression is the name. */
export interface ClassDirective extends Directive {
	type: 'ClassDirective';
	expression: Expression;
}
export interface StyleDirective extends Directive {
	type: 'StyleDirective';
	value: AttributeValue;
}
/** `transition:`, `in:` or `out:`, told apart by `intro` and `outro`. */
export interface TransitionDirective extends Directive {
	type: 'TransitionDirective';
	expression: Expression | null;
	intro: boolean;
	outro: boolean;
}
export interface AnimateDirective extends Directive {
	type: 'AnimateDirective';
	expression: Expression | null;
}
/** `let:name`, or `let:name={pattern}`; without a value the pattern is the name. */
export interface LetDirective extends Directive {
	type: 'LetDirective';
	expression: Pattern;
}

export type DirectiveNode = BindDirective | OnDirective | UseDirective | ClassDirective | StyleDirective | TransitionDirective | AnimateDirective | LetDirective;

/** What an element's attributes list. */
export type AttributeLike = Attribute | SpreadAttribute | AttachTag | DirectiveNode;

// ── tags

/** `{expression}`. */
export interface ExpressionTag extends Base {
	type: 'ExpressionTag';
	expression: Expression;
}
/** `{@html expression}`. */
export interface HtmlTag extends Base {
	type: 'HtmlTag';
	expression: Expression;
}
/** `{@debug a, b}`. */
export interface DebugTag extends Base {
	type: 'DebugTag';
	identifiers: Identifier[];
}
/** `{@const x = y}`. */
export interface ConstTag extends Base {
	type: 'ConstTag';
	declaration: VariableDeclaration;
}
/** `{@render snippet()}`. */
export interface RenderTag extends Base {
	type: 'RenderTag';
	expression: Expression;
}
/** `{@attach expression}` among the attributes. */
export interface AttachTag extends Base {
	type: 'AttachTag';
	expression: Expression;
}
/** `{let x = y}` or `{const x = y}`. */
export interface DeclarationTag extends Base {
	type: 'DeclarationTag';
	declaration: VariableDeclaration;
}

export type Tag = ExpressionTag | HtmlTag | DebugTag | ConstTag | RenderTag | DeclarationTag;

// ── blocks

/** `{#if test}…{:else if test}…{:else}…{/if}`. */
export interface IfBlock extends Base {
	type: 'IfBlock';
	test: Expression;
	consequent: Fragment;
	/** Each `{:else if}`, in order. */
	branches: ElseIf[];
	alternate: Fragment | null;
}
/** One `{:else if test}` and what follows it, up to the next branch or the close. */
export interface ElseIf extends Base {
	type: 'ElseIf';
	test: Expression;
	consequent: Fragment;
}
/** `{#each expression as context, index (key)}…{:else}…{/each}`; `index`, `key` and `fallback` are absent when not written. */
export interface EachBlock extends Base {
	type: 'EachBlock';
	expression: Expression;
	/** Null without `as`. */
	context: Pattern | null;
	index?: Identifier;
	key?: Expression;
	body: Fragment;
	fallback?: Fragment;
}
/** `{#await expression}…{:then value}…{:catch error}…{/await}`, and its shorter spellings. */
export interface AwaitBlock extends Base {
	type: 'AwaitBlock';
	expression: Expression;
	value: Pattern | null;
	then: Fragment | null;
	error: Pattern | null;
	catch: Fragment | null;
	pending: Fragment | null;
}
/** `{#key expression}…{/key}`. */
export interface KeyBlock extends Base {
	type: 'KeyBlock';
	expression: Expression;
	fragment: Fragment;
}
/** `{#snippet name<T>(parameters)}…{/snippet}`; `typeParams` is the text between the angle brackets, absent when not written. */
export interface SnippetBlock extends Base {
	type: 'SnippetBlock';
	expression: Identifier;
	typeParams?: string;
	parameters: Pattern[];
	fragment: Fragment;
}

export type Block = IfBlock | EachBlock | AwaitBlock | KeyBlock | SnippetBlock;

// ── the style element's sheet

/** A style element's sheet: its rules and at-rules, the comments among them, and `content`, the text it was read from. */
export interface StyleSheet extends Base {
	type: 'StyleSheet';
	attributes: Attribute[];
	children: (Rule | Atrule)[];
	comments: CSSComment[];
	content: { start: number; end: number; styles: string; comment: null };
}
/** A comment of the sheet; `position` is its offset into the value it interrupts, absent when it stands between rules. */
export interface CSSComment extends Base {
	type: 'CSSComment';
	value: string;
	position?: number;
}
export interface Rule extends Base {
	type: 'Rule';
	prelude: SelectorList;
	block: CSSBlock;
}
/** `@name prelude { block }`, or `@name prelude;` with a null block. */
export interface Atrule extends Base {
	type: 'Atrule';
	name: string;
	prelude: string;
	block: CSSBlock | null;
}
/** A `{…}` of declarations, rules and at-rules. */
export interface CSSBlock extends Base {
	type: 'Block';
	children: (Declaration | Rule | Atrule)[];
}
export interface Declaration extends Base {
	type: 'Declaration';
	property: string;
	value: string;
}
export interface SelectorList extends Base {
	type: 'SelectorList';
	children: ComplexSelector[];
}
export interface ComplexSelector extends Base {
	type: 'ComplexSelector';
	children: RelativeSelector[];
}
/** The simple selectors a combinator joins to what precedes them; the first has no combinator. */
export interface RelativeSelector extends Base {
	type: 'RelativeSelector';
	combinator: Combinator | null;
	selectors: SimpleSelector[];
}
export interface Combinator extends Base {
	type: 'Combinator';
	name: string;
}
export interface TypeSelector extends Base {
	type: 'TypeSelector';
	name: string;
	/** The part before `|`; absent without one. */
	namespace?: string;
}
export interface IdSelector extends Base {
	type: 'IdSelector';
	name: string;
}
export interface ClassSelector extends Base {
	type: 'ClassSelector';
	name: string;
}
export interface NestingSelector extends Base {
	type: 'NestingSelector';
	name: string;
}
export interface PseudoClassSelector extends Base {
	type: 'PseudoClassSelector';
	name: string;
	args: SelectorList | null;
}
/** `::name`, `args` present when it takes arguments. */
export interface PseudoElementSelector extends Base {
	type: 'PseudoElementSelector';
	name: string;
	args?: SelectorList;
}
export interface AttributeSelector extends Base {
	type: 'AttributeSelector';
	name: string;
	matcher: string | null;
	value: string | null;
	flags: string | null;
}
/** An `An+B` argument of a pseudo-class. */
export interface Nth extends Base {
	type: 'Nth';
	value: string;
}
/** A keyframe selector. */
export interface Percentage extends Base {
	type: 'Percentage';
	value: string;
}

export type SimpleSelector = TypeSelector | IdSelector | ClassSelector | NestingSelector | PseudoClassSelector | PseudoElementSelector | AttributeSelector | Nth | Percentage;

export type CSSNode = StyleSheet | CSSComment | Rule | Atrule | CSSBlock | Declaration | SelectorList | ComplexSelector | RelativeSelector | Combinator | SimpleSelector;

/** Any node of a component but ESTree's. */
export type SvelteNode = Root | Fragment | FragmentNode | Script | AttributeLike | ElseIf | CSSNode;
