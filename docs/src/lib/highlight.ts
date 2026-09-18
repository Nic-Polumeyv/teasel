import { Source, parentOf } from '@teasel/parser';
import type { Node as Estree } from 'estree';

type Node = { type: string; start: number; end: number; [field: string]: unknown };

export type Snippet = { code: string; html: string; file?: string };

type Kind = 'comment' | 'keyword' | 'string' | 'number' | 'type' | 'call' | 'property' | 'operator' | 'punctuation';

const everforest: Record<Kind, string> = {
	comment: 'text-[#859289]',
	keyword: 'text-[#e67e80]',
	string: 'text-[#a7c080]',
	number: 'text-[#d699b6]',
	type: 'text-[#dbbc7f]',
	call: 'text-[#83c092]',
	property: 'text-[#7fbbb3]',
	operator: 'text-[#e69875]',
	punctuation: 'text-[#9da9a0]',
};

const keywords = new Set(
	'import export from as default const let var using function class extends return new if else for of in while do switch case break continue throw try catch finally await async yield typeof instanceof void delete this super static get set constructor declare type interface namespace enum readonly abstract implements keyof infer is satisfies public private protected'.split(
		' ',
	),
);

type Span = { start: number; end: number; kind: Kind };

function kindOf(node: Node): Kind | undefined {
	const parent = parentOf(node as Estree) as Node | undefined;
	switch (node.type) {
		case 'Literal': {
			const value = node.value;
			if (node.regex) return 'string';
			if (typeof value === 'string') return 'string';
			if (typeof value === 'number' || typeof value === 'bigint') return 'number';
			return 'keyword';
		}
		case 'TemplateElement':
			return 'string';
		case 'Identifier':
			if (!parent) return;
			if ((parent.type === 'CallExpression' || parent.type === 'NewExpression') && parent.callee === node) return 'call';
			if (parent.type === 'MemberExpression' && parent.property === node) {
				const outer = parentOf(parent as Estree) as Node | undefined;
				return outer && (outer.type === 'CallExpression' || outer.type === 'NewExpression') && outer.callee === parent ? 'call' : 'property';
			}
			if (parent.type === 'Property' && parent.key === node) return 'property';
			if ((parent.type === 'MethodDefinition' || parent.type === 'TSDeclareMethod') && parent.key === node) return parent.kind === 'constructor' ? 'keyword' : 'call';
			if (parent.type === 'PropertyDefinition' && parent.key === node) return 'property';
			if (parent.type === 'FunctionDeclaration' && parent.id === node) return 'call';
			if (parent.type === 'ClassDeclaration' || parent.type === 'TSInterfaceDeclaration' || parent.type === 'TSTypeAliasDeclaration' || parent.type === 'TSTypeParameter') return 'type';
			if (parent.type.startsWith('TS')) return 'type';
			return;
		default:
			if (/^TS\w+Keyword$/.test(node.type)) return 'type';
	}
}

function collect(node: unknown, spans: Span[]) {
	if (!node || typeof node !== 'object') return;
	if (Array.isArray(node)) {
		for (const child of node) collect(child, spans);
		return;
	}
	const n = node as Node;
	if (typeof n.type === 'string') {
		const kind = kindOf(n);
		if (kind) {
			spans.push({ start: n.start, end: n.end, kind });
			if (kind !== 'call' && kind !== 'property' && kind !== 'type') return;
		}
	}
	for (const key in n) if (key !== 'loc') collect(n[key], spans);
}

const escape = (text: string) => text.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c]!);

function paint(text: string, kind?: Kind) {
	return kind ? `<span class="${everforest[kind]}">${escape(text)}</span>` : escape(text);
}

function between(text: string) {
	let html = '';
	for (const match of text.matchAll(/[A-Za-z_$][\w$]*|\d[\w.]*|\s+|[=+\-*/<>!&|?:^%~]+|[^\w\s$]/g)) {
		const token = match[0];
		if (/^[A-Za-z_$]/.test(token)) html += paint(token, keywords.has(token) ? 'keyword' : undefined);
		else if (/^\d/.test(token)) html += paint(token, 'number');
		else if (/^\s/.test(token)) html += token;
		else if (/^[=+\-*/<>!&|?:^%~]/.test(token)) html += paint(token, 'operator');
		else html += paint(token, 'punctuation');
	}
	return html;
}

function script(code: string, typescript: boolean) {
	const source = new Source(code, { sourceType: 'module', typescript, comments: true, errorRecovery: true });
	try {
		const { node, comments = [] } = source.parse();
		const spans: Span[] = comments.map((c) => ({ start: c.start, end: c.end, kind: 'comment' as const }));
		collect(node, spans);
		spans.sort((a, b) => a.start - b.start || b.end - a.end);
		let html = '';
		let at = 0;
		for (const span of spans) {
			if (span.start < at) continue;
			html += between(code.slice(at, span.start)) + paint(code.slice(span.start, span.end), span.kind);
			at = span.end;
		}
		return html + between(code.slice(at));
	} finally {
		source[Symbol.dispose]();
	}
}

function shell(code: string) {
	let html = '';
	let command = true;
	for (const match of code.matchAll(/#[^\n]*|"[^"]*"|'[^']*'|\$\w+|--?[\w-]+|[&|;]+|\n|\s+|\S+/g)) {
		const token = match[0];
		if (token[0] === '#') html += paint(token, 'comment');
		else if (token[0] === '"' || token[0] === "'") html += paint(token, 'string');
		else if (token[0] === '$') html += paint(token, 'property');
		else if (token[0] === '-') html += paint(token, 'operator');
		else if (/^[&|;\n]/.test(token)) (html += paint(token, 'punctuation')), (command = true);
		else if (/^\s/.test(token)) html += token;
		else (html += paint(token, command ? 'call' : undefined)), (command = false);
	}
	return html;
}

export function snippet(code: string, lang: 'javascript' | 'typescript' | 'bash' = 'javascript', file?: string): Snippet {
	const html = lang === 'bash' ? shell(code) : script(code, lang === 'typescript');
	return { code, html, file };
}

export type Mark = { start: number; end: number; open: string };

/** Wraps ranges of the code in a snippet's html; a range starts and ends between tokens, never inside one. */
export function mark(html: string, marks: Mark[]) {
	const sorted = marks.toSorted((a, b) => a.start - b.start);
	let out = '';
	let at = 0;
	let depth = 0;
	let next = 0;
	let closing: number | undefined;
	const edge = () => {
		if (closing === at) {
			if (depth !== 0) throw new Error(`a note ends inside a token at ${at}`);
			out += '</span>';
			closing = undefined;
		}
		if (sorted[next]?.start === at) {
			if (depth !== 0 || closing !== undefined) throw new Error(`a note starts inside a token or another note at ${at}`);
			out += sorted[next].open;
			closing = sorted[next].end;
			next += 1;
		}
	};
	for (const [piece] of html.matchAll(/<\/span>|<span[^>]*>|&\w+;|[\s\S]/g)) {
		if (piece === '</span>') depth -= 1;
		else {
			edge();
			if (piece.startsWith('<span')) depth += 1;
			else at += 1;
		}
		out += piece;
	}
	edge();
	if (next !== sorted.length || closing !== undefined) throw new Error('a note lies past the end of its code');
	return out;
}
