// The scope and binding tables of an answer, linked into objects: `linkScopes` is a copy of the
// one in `bindings/node/decode.js`, `linkJson` finds the nodes in a tree that came as JSON.

/**
 * Turns the answer's scope and binding tables into objects and the numbers on nodes into
 * references to them: `node.scope`, `identifier.binding`, `binding.node`, `binding.references`,
 * `scope.node`, `scope.parent`, `scope.bindings`, `scope.through`.
 * @param {any} answer a program or a parse-at answer carrying `scopes` and `bindings`
 * @param {any[]} indexed the nodes carrying a `scope`, `declares` or `binding` number
 */
export function linkScopes(answer, indexed) {
	if (!answer || !answer.scopes) return;
	const scopes = answer.scopes.map((scope) => ({ ...scope, node: null, bindings: [], declarations: new Map() }));
	const bindings = answer.bindings.map((binding) => ({ ...binding, node: null, references: [] }));
	for (const scope of scopes) {
		scope.parent = scope.parent === null ? null : scopes[scope.parent];
		scope.through = scope.through.map((index) => bindings[index]);
	}
	for (const binding of bindings) {
		binding.scope = scopes[binding.scope];
		binding.scope.bindings.push(binding);
		binding.scope.declarations.set(binding.name, binding);
	}
	for (const node of indexed) {
		// the tables' own entries carry `scope` too, and are not nodes
		if (typeof node.type !== 'string') continue;
		if (node.scope !== undefined) {
			node.scope = scopes[node.scope];
			node.scope.node = node;
		}
		if (node.declares !== undefined) {
			const binding = bindings[node.declares];
			delete node.declares;
			node.binding = binding;
			if (binding.node === null) binding.node = node;
		} else if (node.binding !== undefined && node.binding !== null) {
			node.binding = bindings[node.binding];
			node.binding.references.push(node);
		}
	}
	answer.scopes = scopes;
	answer.bindings = bindings;
}

/** Finds the indexed nodes by walking, for answers that came as JSON. */
export function linkJson(answer) {
	const indexed = [];
	const seen = new Set();
	const walk = (value) => {
		if (!value || typeof value !== 'object' || seen.has(value)) return;
		seen.add(value);
		if (Array.isArray(value)) {
			for (const item of value) walk(item);
			return;
		}
		if ('scope' in value || 'binding' in value || 'declares' in value) indexed.push(value);
		for (const key in value) {
			if (key !== 'scopes' && key !== 'bindings' && key !== 'loc') walk(value[key]);
		}
	};
	walk(answer);
	linkScopes(answer, indexed);
}
