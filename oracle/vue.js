// Differential test: Vue's template parser versus teasel reading the same template by the Vue
// grammar, over every template the Vue compiler's own tests parse.
//
//   VUE_DIR=~/Projects/vue-core bun vue.js [--verbose] [--limit N] [filter]

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { args, capped, diff, files } from './lib.js';
import { parse as parseVue } from '@vue/compiler-dom';
import { Source } from '../package/index.js';

const { verbose, limit, filter } = args();
const vue = process.env.VUE_DIR ?? join(process.env.HOME, 'Projects/vue-core');
const grammar = readFileSync(new URL('../hosts/vue.grammar', import.meta.url), 'utf8');

// every string a compiler test parses: the literals of the spec files that look like markup
function corpus() {
	const cache = new URL('../target/vue-corpus.json', import.meta.url).pathname;
	if (existsSync(cache)) return JSON.parse(readFileSync(cache, 'utf8'));
	const seen = new Set();
	const out = [];
	for (const pkg of ['compiler-core', 'compiler-dom', 'compiler-sfc', 'compiler-ssr']) {
		for (const path of files(join(vue, 'packages', pkg, '__tests__'), /\.spec\.ts$/)) {
			const name = relative(join(vue, 'packages'), path);
			const text = readFileSync(path, 'utf8');
			let tree;
			try {
				tree = new Source(text, { typescript: true, sourceType: 'module' }).parse().node;
			} catch (e) {
				console.error(`${name}: ${e.message}`);
				continue;
			}
			const found = [];
			(function walk(node) {
				if (Array.isArray(node)) return node.forEach(walk);
				if (!node || typeof node !== 'object') return;
				if (node.type === 'Literal' && typeof node.value === 'string') found.push(node.value);
				else if (node.type === 'TemplateLiteral' && node.expressions.length === 0) found.push(node.quasis[0].value.cooked);
				for (const [k, v] of Object.entries(node)) if (k !== 'loc') walk(v);
			})(tree);
			for (const [i, source] of found.entries()) {
				if (!/<[a-zA-Z!\/]|\{\{/.test(source) || source.length > 20000 || seen.has(source)) continue;
				seen.add(source);
				out.push({ name: `${name}#${i}`, source });
			}
		}
	}
	writeFileSync(cache, JSON.stringify(out));
	return out;
}

// Vue's positions: UTF-16 offsets, one-based lines and columns, and the text between
function locator(source) {
	const starts = [0];
	for (let i = 0; i < source.length; i++) if (source[i] === '\n') starts.push(i + 1);
	const pos = (offset) => {
		let lo = 0, hi = starts.length - 1;
		while (lo < hi) {
			const mid = (lo + hi + 1) >> 1;
			if (starts[mid] <= offset) lo = mid;
			else hi = mid - 1;
		}
		return { offset, line: lo + 1, column: offset - starts[lo] + 1 };
	};
	return (start, end) => ({ start: pos(start), end: pos(end), source: source.slice(start, end) });
}

const DROP = new Set(['ns', 'tagType', 'codegenNode', 'helpers', 'components', 'directives', 'hoists', 'imports', 'cached', 'temps', 'constType', 'ast', 'nameLoc', 'finalized']);

// Vue's tree with what codegen adds taken out; expressions as their trimmed text
function normalVue(value, key, loc) {
	if (Array.isArray(value)) return value.map((v) => normalVue(v, key, loc));
	if (!value || typeof value !== 'object') return value;
	if (value.type === 4) {
		const trimmed = value.content.trim();
		// an empty expression is none
		if (key === 'exp' && trimmed === '') return undefined;
		const lead = value.content.length - value.content.trimStart().length;
		const trail = value.content.length - value.content.trimEnd().length;
		const out = { type: 4, content: trimmed, isStatic: value.isStatic };
		if ((key === 'exp' || key === 'content') && value.loc.source !== '') out.loc = loc(value.loc.start.offset + lead, value.loc.end.offset - trail);
		return out;
	}
	const out = {};
	for (const [k, v] of Object.entries(value)) {
		if (DROP.has(k) || v === undefined) continue;
		const normal = normalVue(v, k, loc);
		if (normal !== undefined) out[k] = normal;
	}
	return out;
}

function expected(source) {
	const errors = [];
	let ast;
	try {
		ast = parseVue(source, { comments: true, onError: (e) => errors.push(e) });
	} catch (e) {
		return { error: { message: e.message, code: e.code } };
	}
	if (errors.length) return { error: { message: errors[0].message, code: errors[0].code, pos: errors[0].loc?.start.offset } };
	return normalVue(ast, undefined, locator(source));
}

const PRE_NEWLINE = /^\r?\n/;
const isWhitespace = (c) => c === ' ' || c === '\n' || c === '\t' || c === '\r' || c === '\f';
function condense(str) {
	let ret = '';
	let prev = false;
	for (const c of str) {
		if (isWhitespace(c)) {
			if (!prev) ret += ' ';
			prev = true;
		} else {
			ret += c;
			prev = false;
		}
	}
	return ret;
}
const allWhitespace = (s) => [...s].every(isWhitespace);
// Vue's whitespace handling, run on a mapped children list
function condenseWhitespace(nodes, inPre) {
	let removed = false;
	for (let i = 0; i < nodes.length; i++) {
		const node = nodes[i];
		if (node.type !== 2) continue;
		if (!inPre) {
			if (allWhitespace(node.content)) {
				const prev = nodes[i - 1]?.type;
				const next = nodes[i + 1]?.type;
				if (!prev || !next || (prev === 3 && (next === 3 || next === 1)) || (prev === 1 && (next === 3 || (next === 1 && /[\r\n]/.test(node.content))))) {
					removed = true;
					nodes[i] = null;
				} else node.content = ' ';
			} else node.content = condense(node.content);
		} else node.content = node.content.replace(/\r\n/g, '\n');
	}
	return removed ? nodes.filter(Boolean) : nodes;
}

// teasel's tree in Vue's shape
function ours(root, source) {
	const loc = locator(source);
	const exp = (node, key) => (node === null || node === undefined ? undefined : { type: 4, content: source.slice(node.start, node.end), isStatic: false, ...(key === 'exp' && { loc: loc(node.start, node.end) }) });
	let inPre = 0;
	function map(node) {
		switch (node.type) {
			case 'Text':
				return { type: 2, content: node.content, loc: loc(node.start, node.end) };
			case 'Comment':
				return { type: 3, content: node.content, loc: loc(node.start, node.end) };
			case 'Interpolation': {
				let a = node.start + 2, b = node.end - 2;
				while (a < b && isWhitespace(source[a])) a++;
				while (b > a && isWhitespace(source[b - 1])) b--;
				return { type: 5, content: { type: 4, content: source.slice(a, b), isStatic: false, loc: loc(a, b) }, loc: loc(node.start, node.end) };
			}
			case 'Attribute': {
				const out = { type: 6, name: node.name, loc: loc(node.start, node.end) };
				if (node.value !== true) {
					const chunk = node.value[0];
					const quoted = source[chunk.start - 1] === '"' || source[chunk.start - 1] === "'";
					out.value = { type: 2, content: node.name === 'class' ? condense(chunk.content).trim() : chunk.content, loc: quoted ? loc(chunk.start - 1, chunk.end + 1) : loc(chunk.start, chunk.end) };
				}
				return out;
			}
			case 'Directive': {
				// the `.prop` of the `.` shorthand is one Vue makes without a position, and not static
				const out = { type: 7, name: node.name, rawName: node.rawName, modifiers: node.modifiers.map((m, i) => ({ type: 4, content: m, isStatic: !(i === 0 && m === 'prop' && node.rawName.startsWith('.')) })), loc: loc(node.start, node.end) };
				if (node.arg !== null) out.arg = typeof node.arg === 'string' ? { type: 4, content: node.arg, isStatic: true } : { type: 4, content: source.slice(node.arg.start, node.arg.end), isStatic: false };
				// a slot name may hold dots, which Vue reads back out of the modifiers
				if (node.name === 'slot' && out.arg && out.modifiers.length) {
					out.arg.content += '.' + out.modifiers.map((m) => m.content).join('.');
					out.modifiers = [];
				}
				// the value's text between the quotes, trimmed, is what Vue keeps as the expression
				const eq = source.indexOf('=', node.start + node.rawName.length);
				if (eq !== -1 && eq < node.end) {
					let a = eq + 1;
					while (isWhitespace(source[a])) a++;
					const quote = source[a] === '"' || source[a] === "'" ? source[a] : null;
					if (quote) a++;
					let b = quote ? source.indexOf(quote, a) : node.end;
					const text = source.slice(a, b);
					const lead = text.length - text.trimStart().length;
					const trail = text.length - text.trimEnd().length;
					if (text.trim() !== '') out.exp = { type: 4, content: text.trim(), isStatic: false, loc: loc(a + lead, b - trail) };
				}
				if ('source' in node && node.source !== null) {
					out.forParseResult = { source: exp(node.source), value: exp(node.value), key: exp(node.key), index: exp(node.index) };
					for (const k of ['value', 'key', 'index']) if (out.forParseResult[k] === undefined) delete out.forParseResult[k];
				}
				return out;
			}
			default: {
				const pre = node.tag === 'pre';
				if (pre) inPre++;
				const children = node.children.map(map);
				if (pre) inPre--;
				// raw text and rich text elements keep their whitespace
				const raw = ['script', 'style', 'textarea', 'title'].includes(node.tag);
				// an empty raw element has no text node in Vue
				const out = { type: 1, tag: node.tag, props: node.props.filter((p) => (p.rawName ?? p.name) !== 'v-pre').map(map), children: raw ? children.filter((c) => c.content !== '') : condenseWhitespace(children, inPre || pre), loc: loc(node.start, node.end) };
				if ((node.tag === 'pre' || node.tag === 'textarea') && out.children[0]?.type === 2) out.children[0].content = out.children[0].content.replace(PRE_NEWLINE, '');
				if (source.slice(node.end - 2, node.end) === '/>') out.isSelfClosing = true;
				return out;
			}
		}
	}
	return { type: 0, source, children: condenseWhitespace(root.children.map(map), 0), loc: loc(0, source.length) };
}

const jobs = corpus().filter((job) => !filter || job.name.includes(filter));
capped(jobs, limit);
const HOST_CODES = new Set(['expected', 'unclosed', 'unexpected_close', 'invalid_name', 'duplicate', 'placement']);
const stats = { identical: 0, mismatch: 0, both_error: 0, only_vue_error: 0, only_teasel_error: 0, expression_error: 0 };
const details = [];
for (const job of jobs) {
	const e = expected(job.source);
	let a;
	try {
		a = ours(new Source(job.source, { host: grammar, sourceType: 'module', typescript: true }).parse().node, job.source);
	} catch (error) {
		a = { error: { message: error.message, code: error.code, pos: error.pos } };
	}
	if (e.error && a.error) stats.both_error++;
	else if (a.error && !HOST_CODES.has(a.error.code)) stats.expression_error++;
	else if (e.error) {
		stats.only_vue_error++;
		details.push(`${job.name}: vue error "${e.error.message}" @${e.error.pos}, teasel parsed`);
	} else if (a.error) {
		stats.only_teasel_error++;
		details.push(`${job.name}: teasel error "${a.error.message}" @${a.error.pos}, vue parsed: ${JSON.stringify(job.source).slice(0, 100)}`);
	} else {
		const d = diff(e, a);
		if (d) {
			stats.mismatch++;
			details.push(`${job.name}: ${d}`);
		} else stats.identical++;
	}
}
console.log(`${jobs.length} templates from the Vue compiler tests`);
for (const [k, v] of Object.entries(stats)) console.log(`  ${k.padEnd(18)} ${v}`);
const agree = stats.identical + stats.both_error + stats.expression_error;
console.log(`  agreement          ${((100 * agree) / jobs.length).toFixed(2)}%`);
const shown = verbose ? details : details.slice(0, 40);
for (const line of shown) console.log(line);
if (!verbose && details.length > shown.length) console.log(`... ${details.length - shown.length} more (--verbose)`);
process.exit(jobs.length > 0 && agree === jobs.length ? 0 : 1);
