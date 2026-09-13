declare const out: unique symbol;
export type JSONValue = null | boolean | number | string | JSONValue[] | {
    [key: string]: JSONValue;
};
export type JS = 'expression' | 'assignmentExpression' | 'pattern' | 'bindingIdentifier' | 'identifierReference' | 'params' | 'typeParameters' | 'statement' | 'program';
export type Mode = 'normal' | 'raw' | 'rcdata' | 'verbatim';
export type Stop = {
    prefixes: string[];
} | {
    matchingElement: true;
} | {
    documentEnd: true;
};
export type Reader = {
    kind: 'token';
    text: string;
    gap: 'space*' | 'none';
    word: boolean;
} | {
    kind: 'space';
    min: number;
} | {
    kind: 'test';
    value: V<boolean>;
} | {
    kind: 'rule';
    name: string;
} | {
    kind: 'javascript';
    entry: JS;
    boundary?: 'last-shared-word';
} | {
    kind: 'html-single';
    entry: JS;
} | {
    kind: 'html-attributes';
    mode: 'normal' | 'static';
} | {
    kind: 'html-attribute-parts';
} | {
    kind: 'html-children';
    mode: Mode;
    stop: Stop;
} | {
    kind: 'css-stylesheet';
};
export interface V<out T = unknown> {
    readonly [out]: T;
}
export type Expr = V & {
    readonly name?: never;
};
export interface Slot<out N extends string> extends V<any> {
    readonly name: N;
}
export type Schema = Record<string, 'null' | 'omit'>;
type Names<S extends Schema, L extends readonly string[]> = (keyof S & string) | L[number];
type Slots<S extends Schema, L extends readonly string[]> = {
    readonly [N in Names<S, L>]: Slot<N>;
};
export interface Form<out W = unknown> {
    readonly [out]: W;
}
export type Write<N extends string, K extends string> = {
    into: N;
    kind: K;
};
type Wrote<Fs extends readonly Form[]> = Form<Fs[number][typeof out]>;
type Kind<R extends Reader> = R extends {
    kind: 'javascript' | 'html-single';
    entry: infer E;
} ? E : R['kind'];
type Bindable<W> = W extends Write<infer N, 'pattern' | 'bindingIdentifier' | 'params' | 'value'> ? N : never;
export interface Region<out Id extends string, out F extends string> {
    readonly id: Id;
    readonly parent: V;
    readonly kind: 'module' | 'script' | 'fragment' | 'block' | 'function';
    readonly covers: V;
    readonly when?: V<boolean>;
    readonly each?: {
        list: V;
        as: string;
    };
    readonly [out]: F;
}
export interface Declare<out N extends string, out R extends string | Expr> {
    readonly patterns: V;
    readonly into: V;
    readonly kind: 'pattern' | 'param';
    readonly [out]: [N, R];
}
export interface RuleData {
    readonly type: string;
    readonly fields: Schema;
    readonly locals?: readonly string[];
    form: Form;
    regions?: readonly Region<string, string>[];
    declares?: readonly Declare<string, string | Expr>[];
    span?: 'none' | 'through-next-token-start';
}
export type Dispatch = {
    when: V<boolean>;
    rule: string;
    type?: string;
    attributes?: 'static';
    content?: Mode;
};
export type Plan = {
    version: 1;
    document: string;
    rules: Record<string, {
        toJSON(): RuleData;
    }>;
    html: {
        delimiters: [string, string];
        attributeInterpolations: boolean;
        attributeComments: 'javascript' | 'none';
        autoclose: boolean;
        trimEnd: boolean;
        void: string[];
        text: string;
        comment: string;
        content: {
            prefix: string;
            rule: string;
        }[];
        attribute: {
            prefix: string;
            rule: string;
        }[];
        plainAttribute: {
            type: string;
            name: string;
            value: string;
            text: string;
            expression: string;
        };
        elements: Dispatch[];
        directiveNames: {
            prefix: string;
            argument: string;
            modifier: string;
            requireArgument: boolean;
            dynamic: [string, string] | null;
            unknown: 'plain-attribute' | 'wildcard-rule';
        };
        directives: {
            name: string;
            rule: string;
        }[];
    };
};
export declare const constant: <const T extends JSONValue>(value: T) => V<T>;
export declare const get: (base: string | V, ...path: (string | number)[]) => V<any>;
export declare const equal: (left: V, right: V) => V<boolean>;
export declare const less: (left: V, right: V) => V<boolean>;
export declare const present: (left: V) => V<boolean>;
export declare const choose: <A, B>(condition: V<boolean>, yes: V<A>, no: V<B>) => V<A | B>;
export declare const flatMap: (list: V, as: string, body: V) => V<unknown[]>;
export declare const length: (list: V) => V<number>;
export declare const at: (list: V, index: number) => V<any>;
export declare const array: (...items: V[]) => V<unknown[]>;
export declare const record: (type: string | null, fields: Record<string, V>, span?: V) => V;
export declare const not: (p: V<boolean>) => V<boolean>;
export declare const and: (a: V<boolean>, b: V<boolean>) => V<boolean>;
export declare const or: (a: V<boolean>, b: V<boolean>) => V<boolean>;
export declare const filter: (list: V, as: string, predicate: V<boolean>) => V<unknown[]>;
export declare const map: (list: V, as: string, value: V) => V<unknown[]>;
export declare const any: (list: V) => V<boolean>;
export declare const member: (value: V, literals: JSONValue[]) => V<boolean>;
export declare const concat: (...lists: V[]) => V<unknown[]>;
export declare const scope: (name: string) => V<any>;
export declare const incoming: V<any>;
export declare const isType: (value: V, ...types: string[]) => V<boolean>;
export declare const attributes: (node: V) => V<unknown[]>;
export declare const attr: (node: V, name: string) => V<unknown[]>;
export declare const hasAttribute: (node: V, name: string) => V<boolean>;
export declare const staticAttribute: (node: V, name: string) => V<any>;
export declare const seq: <Fs extends readonly Form[]>(...items: Fs) => Wrote<Fs>;
export declare const choice: <Fs extends readonly Form[]>(...alternatives: Fs) => Wrote<Fs>;
export declare const optional: <F extends Form>(form: F) => F;
export declare const read: <R extends Reader, N extends string = never>(reader: R, into?: Slot<N>, input?: V) => Form<Write<N, Kind<R>>>;
export declare const emit: <N extends string>(into: Slot<N>, value: V) => Form<Write<N, "value">>;
export declare const token: (text: string, tight?: boolean) => Form<Write<never, "token">>;
export declare const space: () => Form<Write<never, "space">>;
export declare const test: (value: V<boolean>) => Form<Write<never, "test">>;
export declare const call: <N extends string = never>(name: string, into?: Slot<N>) => Form<Write<N, "rule">>;
export declare const js: <E extends JS, N extends string = never>(entry: E, into?: Slot<N>, input?: V, boundary?: "last-shared-word") => Form<Write<N, E>>;
export declare const repeat: <N extends string>(build: (r: {
    readonly item: Slot<"item">;
}) => Form, into: Slot<N>, value?: V, min?: number, max?: number | null) => Form<Write<N, "repeat">>;
export declare const many: <N extends string>(name: string, into: Slot<N>, min: number, max: number | null) => Form<Write<N, "repeat">>;
export declare const region: <const Id extends string, F extends string>(id: Id, parent: V, covers: readonly (Slot<F> | Expr)[] | Expr, kind?: Region<Id, F>["kind"], when?: V<boolean>) => Region<Id, F>;
export declare const declare: <N extends string, const R extends string | Expr>(patterns: readonly Slot<N>[] | Expr, into: R, kind?: "pattern" | "param") => Declare<N, R>;
export declare class Rule<Type extends string, S extends Schema, L extends readonly string[], W, R extends string> {
    #private;
    readonly [out]: {
        type: Type;
        fields: S;
        writes: W;
    };
    constructor(type: Type, fields: S, locals: L);
    form<W2 extends Write<Names<S, L>, string>>(build: (f: Slots<S, L>) => Form<W2>): Rule<Type, S, L, W2, R>;
    regions<const Rs extends readonly Region<string, Names<S, L>>[]>(build: (f: Slots<S, L>) => Rs): Rule<Type, S, L, W, R | Rs[number]['id']>;
    declares(build: (f: Slots<S, L>) => readonly Declare<Bindable<W>, R | 'incoming' | Expr>[]): this;
    span(policy: RuleData['span']): this;
    toJSON(): RuleData;
}
export declare const rule: <const Type extends string, const S extends Schema, const L extends readonly string[] = []>(type: Type, fields: S, locals?: L) => Rule<Type, S, L, never, never>;
interface Nodes {
    expression: import('estree').Expression;
    assignmentExpression: import('estree').Expression;
    pattern: import('estree').Pattern;
    bindingIdentifier: import('estree').Identifier;
    identifierReference: import('estree').Identifier;
    params: import('estree').Pattern[];
    statement: import('estree').Statement;
    program: import('estree').Program;
}
type Kinds<W, N> = W extends Write<infer M, infer K> ? ([M] extends [never] ? never : M extends N ? K : never) : never;
type Node<W, N> = Kinds<W, N> extends infer K ? (K extends keyof Nodes ? Nodes[K] : unknown) : never;
/** The node a rule produces: `null` fields are required and nullable, `omit` fields optional. */
export type Infer<Rl extends {
    readonly [out]: {
        type: string;
        fields: Schema;
        writes: unknown;
    };
}> = {
    type: Rl[typeof out]['type'];
    start: number;
    end: number;
} & {
    [N in keyof Rl[typeof out]['fields'] as Rl[typeof out]['fields'][N] extends 'null' ? N : never]: Node<Rl[typeof out]['writes'], N> | null;
} & {
    [N in keyof Rl[typeof out]['fields'] as Rl[typeof out]['fields'][N] extends 'omit' ? N : never]?: Node<Rl[typeof out]['writes'], N>;
};
export {};
