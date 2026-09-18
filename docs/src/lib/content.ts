import { Marked, type Token, type Tokens } from 'marked';
import type { Component } from 'svelte';
import { render as ssr } from 'svelte/server';
import { buttonVariants } from 'sheer-ui/components/button';
import { mark, snippet, type Mark } from '#lib/highlight.ts';
import { reference } from '#lib/reference.ts';

export type Heading = { id: string; text: string };
export type Page = { href: string; title: string; section: string; path: string; headings: Heading[] };
export type Section = { label: string; pages: Page[] };

export const slug = (text: string) => text.toLowerCase().replace(/[^\w]+/g, '-').replace(/^-|-$/g, '');

// two exports that differ only by case, Entry and ENTRY, would share one id and one each key
const ids = new WeakMap<Tokens.Heading, string>();
function label_headings(headings: Tokens.Heading[]) {
	const seen = new Map<string, number>();
	for (const token of headings) {
		const base = slug(token.text);
		const n = seen.get(base) ?? 0;
		seen.set(base, n + 1);
		ids.set(token, n ? `${base}-${n}` : base);
	}
}

const escape = (text: string) => text.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

const copy = buttonVariants({ variant: 'ghost', size: 'icon', class: 'group size-7 text-white/50 hover:bg-white/10 hover:text-white' });
const copier = (text: string, place = '') =>
	`<button type="button" data-copy="${escape(text)}" title="Copy to clipboard" aria-label="Copy to clipboard" class="${copy} ${place}">` +
	`<svg viewBox="0 0 24 24" class="size-4 group-data-copied:hidden" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 0 1 2-2h9"/></svg>` +
	`<svg viewBox="0 0 24 24" class="hidden size-4 group-data-copied:block" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg></button>`;

const sources = Object.fromEntries(Object.entries(import.meta.glob('/content/**/*.js', { query: '?raw', import: 'default', eager: true })).map(([path, text]) => [path.slice(path.lastIndexOf('/') + 1), text as string]));

/** A whole file the page is about, shown as the sheet it is: a name tab, every line numbered, scrolling past a screen's worth. */
function sheet(name: string) {
	const text = sources[name];
	if (text === undefined) throw new Error(`no ${name} under content/`);
	const lines = snippet(text.replace(/\n$/, ''), 'javascript').html.split('\n');
	return (
		`<figure class="my-8 overflow-hidden rounded-sm bg-[#2d353b] text-[#d3c6aa] shadow-lg">` +
		`<figcaption class="flex h-10 items-center gap-2 border-b border-dashed border-[#d3c6aa]/25 px-4 font-serif text-[15px] text-[#d3c6aa]/85 italic">${escape(name)}<span class="ml-auto font-sans text-xs text-white/40 not-italic">${lines.length} lines</span></figcaption>` +
		`<pre class="max-h-[38rem] overflow-y-auto py-4 pr-4 font-mono text-xs leading-6 whitespace-pre-wrap [overflow-wrap:anywhere] [tab-size:2] sm:text-[13px]"><code>${lines.map((line, i) => `<span class="inline-block w-12 select-none pr-4 text-right text-white/30">${i + 1}</span>${line}`).join('\n')}</code></pre></figure>`
	);
}

const diagrams = Object.fromEntries(Object.entries(import.meta.glob('/content/**/*.svelte', { import: 'default', eager: true })).map(([path, diagram]) => [path.slice(path.lastIndexOf('/') + 1), { path, diagram: diagram as Component<{ label: string; files?: Record<string, string> }> }]));

// names the package exports, and where the reference describes each
const described: Record<string, string> = { Source: 'source', Plan: 'plan', Options: 'options', ParseError: 'parseerror', Parsed: 'parsed', Scope: 'scope', Binding: 'binding', Reference: 'reference', Root: 'root', HostNode: 'hostnode', Code: 'code', referenceOf: 'referenceof', scopeOf: 'scopeof', parentOf: 'parentof', until: 'plan' };

const noted = 'cursor-help rounded-xs underline decoration-white/40 decoration-dotted underline-offset-4 outline-none hover:bg-white/10 focus-visible:bg-white/10';

/** The notes of a code block: a line each, the text in the code it is about, `@2` for its second occurrence, `::`, then what to say. */
function marks(code: string, notes: string): Mark[] {
	return notes
		.split('\n')
		.filter((line) => line.trim() !== '')
		.map((line) => {
			const [, key, nth = '1', body] = /^(.+?)(?: @(\d+))? :: (.+)$/.exec(line) ?? [];
			if (body === undefined) throw new Error(`a note reads "text :: what to say", not "${line}"`);
			let start = -1;
			for (let n = 0; n < +nth; n++) start = code.indexOf(key, start + 1);
			if (start === -1) throw new Error(`no "${key}" in the code the note is about`);
			return { start, end: start + key.length, open: `<span tabindex="0" role="button" data-note="${escape(marked.parseInline(body) as string)}" class="${noted}">` };
		});
}

export function fence(text: string, language = 'js', file?: string, notes?: string) {
	const plain = language === 'text' ? escape(text) : snippet(text, language === 'ts' || language === 'typescript' ? 'typescript' : language === 'bash' || language === 'sh' ? 'bash' : 'javascript').html;
	const html = notes ? mark(plain, marks(text, notes)) : plain;
	return (
		`<div class="relative -mx-4 my-6 overflow-hidden bg-[#2d353b] text-xs text-[#d3c6aa] shadow-md sm:mx-0 sm:rounded-sm sm:text-sm">` +
		(file ? `<div class="flex h-10 items-center gap-2 border-b border-dashed border-[#d3c6aa]/25 px-4"><span class="flex-1 truncate font-serif text-[15px] text-[#d3c6aa]/85 italic">${escape(file)}</span>${copier(text, '-mr-2')}</div>` : copier(text, 'absolute top-1.5 right-1.5 bg-[#2d353b]')) +
		`<pre class="px-4 py-4 font-mono leading-6 whitespace-pre-wrap [overflow-wrap:anywhere] [tab-size:2]"><code>${html}</code></pre></div>`
	);
}

const marked = new Marked({
	renderer: {
		// a diagram is a component rendered here; one with a title is interactive: the title names the files it
		// shows, and the page hydrates it in the browser with the same props
		image({ href, text, title }) {
			const found = diagrams[href];
			if (!found) return `<img src="${href}" alt="${escape(text)}">`;
			if (!title) return ssr(found.diagram, { props: { label: text } }).body;
			const props = { label: text, files: Object.fromEntries(title.split(/\s+/).map((name) => [name, sheet(name)])) };
			return `<div data-island="${escape(found.path)}" data-props="${escape(JSON.stringify(props))}">${ssr(found.diagram, { props }).body}</div>`;
		},
		code(token) {
			const [language, file] = (token.lang ?? '').split(/\s+/);
			return language === 'file' ? sheet(file) : fence(token.text, language, file, notes.get(token));
		},
		codespan(token) {
			const name = token.text.replace(/\(.*\)$/, '');
			const id = described[name] ?? (name.startsWith('Plan.') ? 'plan' : name === 'source.parse' ? 'source' : undefined);
			const code = `<code>${escape(token.text)}</code>`;
			return id === undefined || linked.has(token) ? code : `<a href="/reference/parser#${id}" class="decoration-dotted!">${code}</a>`;
		},
		heading(token) {
			return `<h${token.depth} id="${ids.get(token)}">${this.parser.parseInline(token.tokens)}</h${token.depth}>`;
		},
	},
});

const tokens = new Map<string, Token[]>();
const notes = new WeakMap<Tokens.Code, string>();
const linked = new WeakSet<Token>();

// a `notes` block belongs to the code block before it
function attach(lexed: Token[]) {
	for (let i = lexed.length - 1; i > 0; i--) {
		const token = lexed[i];
		if (token.type !== 'code' || token.lang !== 'notes') continue;
		let j = i - 1;
		while (j >= 0 && lexed[j].type === 'space') j -= 1;
		if (lexed[j]?.type !== 'code') throw new Error('notes follow the code block they are about');
		notes.set(lexed[j] as Tokens.Code, token.text);
		lexed.splice(j + 1, i - j);
	}
}

function page(meta: Omit<Page, 'headings'>, markdown: string): Page {
	const lexed = marked.lexer(markdown);
	attach(lexed);
	tokens.set(meta.href, lexed);
	const headings: Tokens.Heading[] = [];
	marked.walkTokens(lexed, (token) => {
		if (token.type === 'heading') headings.push(token as Tokens.Heading);
		if (token.type === 'link' || token.type === 'heading') marked.walkTokens(token.tokens ?? [], (inner) => void linked.add(inner));
	});
	label_headings(headings);
	return { ...meta, headings: headings.filter((token) => token.depth === 2).map((token) => ({ id: ids.get(token)!, text: token.text })) };
}

export const render = (page: Page) => marked.parser(tokens.get(page.href)!);

const name = (segment: string) => segment.replace(/^\d+-/, '');
const label = (segment: string) => name(segment).replace(/-/g, ' ').replace(/^\w/, (c) => c.toUpperCase());

const files = import.meta.glob('/content/**/*.md', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

const written = Object.keys(files)
	.sort()
	.map((path) => {
		const [, section, file] = /\/content\/([^/]+)\/([^/]+)\.md$/.exec(path)!;
		const [, front = '', markdown = files[path]] = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/.exec(files[path]) ?? [];
		const title = /^title:\s*(.+)$/m.exec(front)?.[1] ?? label(file);
		return page({ href: `/${name(file)}`, title, section: label(section), path: `docs${path}` }, markdown);
	});

export const pages: Page[] = [...written, page(reference.meta, reference.markdown)];

export const sections: Section[] = Object.entries(Object.groupBy(pages, (page) => page.section)).map(([label, pages]) => ({ label, pages: pages! }));
