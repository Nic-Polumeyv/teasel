use teasel::Options;
use teasel::ast::{Ast, NodeId, NodeKind, Value};
use teasel::host::{self, plan::Plan};

fn field(ast: &Ast, node: NodeId, name: &str) -> Option<Value> {
	let NodeKind::Host(index) = ast.node(node).kind else {
		return None;
	};
	let host = ast.hosts[index as usize];
	ast.host_fields[host.fields.0 as usize..(host.fields.0 + host.fields.1) as usize]
		.iter()
		.find(|(key, _)| *key == name)
		.map(|(_, value)| *value)
}

#[test]
fn native_entries_and_html() {
	let plan = Plan::read(include_str!("hosts/vue/plan.json")).unwrap();
	let (ast, root) = host::parse("<p title=\"a&amp;b\">hello {{name}}</p>", &plan, Options::default());
	let root = root.unwrap();
	let Some(Value::Nodes(children)) = field(&ast, root, "children") else {
		panic!()
	};
	let element = ast.nth(children, 0).unwrap();
	let Some(Value::Nodes(children)) = field(&ast, element, "children") else {
		panic!()
	};
	assert_eq!(children.len, 2);
	let interpolation = ast.nth(children, 1).unwrap();
	let Some(Value::Node(expression)) = field(&ast, interpolation, "content") else {
		panic!()
	};
	assert!(matches!(ast.node(expression).kind, NodeKind::Identifier { .. }));
}

fn document(rule: &str) -> Plan {
	let text = format!(
		r#"{{"version":1,"document":"Document","rules":{{
        "Document":{rule},
        "Text":{{"type":"Text","fields":{{}},"form":{{"op":"seq","items":[]}}}},
        "Comment":{{"type":"Comment","fields":{{}},"form":{{"op":"seq","items":[]}}}}
    }},"html":{{"delimiters":["{{","}}"],"attributeInterpolations":false,"attributeComments":"none",
        "autoclose":false,"trimEnd":false,"typescript":[],"void":[],"text":"Text","comment":"Comment","content":[],"attribute":[],
        "plainAttribute":{{"type":"Attribute","name":"name","value":"value","text":"Text","expression":"Text"}},
        "elements":[],"directiveNames":{{"prefix":"","argument":":","modifier":"|","requireArgument":false,"dynamic":null,"unknown":"plain-attribute"}},"directives":[]
    }}}}"#
	);
	let text = if rule.contains("\"kind\":\"token\"") {
		text.replace("\"document\":\"Document\"", "\"document\":\"Shell\"")
            .replace("\"rules\":{", r#""rules":{"Shell":{"type":"Shell","fields":{"children":"null"},"form":{"op":"read","reader":{"kind":"html-children","mode":"normal","stop":{"documentEnd":true}},"into":"children"}},"#)
            .replace("\"content\":[]", r#""content":[{"prefix":"x","rule":"Document"}]"#)
	} else {
		text
	};
	Plan::read(&text).unwrap()
}

#[test]
fn absence_and_values() {
	let plan = document(
		r#"{"type":"Root","fields":{"missing":"omit","nil":"null","empty":"null","flag":"null","count":"null","selected":"null"},"form":{"op":"seq","items":[
        {"op":"emit","into":"missing","value":{"op":"at","list":{"op":"constant","value":[]},"index":{"op":"constant","value":0}}},
        {"op":"emit","into":"empty","value":{"op":"constant","value":""}},
        {"op":"emit","into":"flag","value":{"op":"constant","value":false}},
        {"op":"emit","into":"count","value":{"op":"length","list":{"op":"flatMap","list":{"op":"constant","value":[1,2,3]},"as":"item","body":{"op":"choose","condition":{"op":"compare","relation":"less","left":{"op":"get","base":"item","path":[]},"right":{"op":"constant","value":3}},"yes":{"op":"construct","shape":"array","items":[{"op":"get","base":"item","path":[]}]},"no":{"op":"constant","value":[]}}}}},
        {"op":"emit","into":"selected","value":{"op":"get","base":{"op":"constant","value":{"x":null}},"path":["x"]}}
    ]}}"#,
	);
	let (ast, root) = host::parse("", &plan, Options::default());
	let root = root.unwrap();
	assert_eq!(field(&ast, root, "missing"), None);
	assert_eq!(field(&ast, root, "nil"), Some(Value::Null));
	assert_eq!(field(&ast, root, "selected"), Some(Value::Null));
	assert_eq!(field(&ast, root, "flag"), Some(Value::Bool(false)));
	assert_eq!(field(&ast, root, "count"), Some(Value::Int(2)));
	let Some(Value::Str(empty)) = field(&ast, root, "empty") else {
		panic!()
	};
	assert_eq!(ast.str(empty), "");
}

#[test]
fn document_comments_are_a_value() {
	let plan = document(
		r#"{"type":"Root","fields":{"comments":"null"},"form":{"op":"emit","into":"comments","value":{"op":"get","base":"event","path":["comments"]}}}"#,
	);
	let (ast, root) = host::parse("", &plan, Options::default());
	assert_eq!(field(&ast, root.unwrap(), "comments"), Some(Value::Comments));
	assert!(ast.comments.is_empty());
}

#[test]
fn repeat_uses_fresh_slots() {
	let plan = document(
		r#"{"type":"Root","fields":{"items":"null"},"form":{"op":"repeat","min":2,"max":2,"locals":["item"],"into":"items","body":{"op":"read","reader":{"kind":"token","text":"x","word":true,"gap":"space*"},"into":"item"},"yield":{"op":"get","base":"iteration","path":["item","text"]}}}"#,
	);
	let (ast, root) = host::parse("x x", &plan, Options::default());
	let Some(Value::Nodes(children)) = field(&ast, root.unwrap(), "children") else {
		panic!()
	};
	let Some(Value::Strs(_, len)) = field(&ast, ast.nth(children, 0).unwrap(), "items") else {
		panic!()
	};
	assert_eq!(len, 2);
}

#[test]
fn choice_rolls_back_slots_nodes_and_comments() {
	let plan = document(
		r#"{"type":"Root","fields":{"failed":"omit","value":"null"},"form":{"op":"choice","alternatives":[
        {"op":"seq","items":[{"op":"read","reader":{"kind":"token","text":"x","word":true,"gap":"none"}},{"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"failed"},{"op":"read","reader":{"kind":"token","text":"z","word":true,"gap":"space*"}}]},
        {"op":"seq","items":[{"op":"read","reader":{"kind":"token","text":"x","word":true,"gap":"none"}},{"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"value"},{"op":"read","reader":{"kind":"token","text":"y","word":true,"gap":"space*"}}]}
    ]}}"#,
	);
	for recover in [false, true] {
		let (ast, root) = host::parse(
			"x foo /*once*/ y",
			&plan,
			Options {
				error_recovery: recover,
				..Default::default()
			},
		);
		let root = root.unwrap();
		let Some(Value::Nodes(children)) = field(&ast, root, "children") else {
			panic!()
		};
		let node = ast.nth(children, 0).unwrap();
		assert_eq!(field(&ast, node, "failed"), None);
		assert!(matches!(field(&ast, node, "value"), Some(Value::Node(_))));
		assert_eq!(ast.comments.len(), 1);
		assert!(ast.errors.is_empty());
		assert_eq!(
			ast.nodes
				.iter()
				.filter(|n| matches!(n.kind, NodeKind::Identifier { .. }))
				.count(),
			1
		);
	}
}

#[test]
fn a_committed_choice_is_not_reopened() {
	let plan = document(
		r#"{"type":"Root","fields":{},"form":{"op":"seq","items":[
        {"op":"choice","alternatives":[{"op":"read","reader":{"kind":"token","text":"x","word":false,"gap":"none"}},{"op":"read","reader":{"kind":"token","text":"xy","word":false,"gap":"none"}}]},
        {"op":"read","reader":{"kind":"token","text":"z","word":false,"gap":"none"}}
    ]}}"#,
	);
	assert!(host::parse("xyz", &plan, Options::default()).1.is_err());
}

#[test]
fn supplied_svelte_forms() {
	let plan = Plan::read(include_str!("hosts/svelte/plan.json")).unwrap();
	for source in [
		"{#if x}a{:else if y}b{:else}c{/if}",
		"{#each xs as {x}, i (i)}{x}{:else}empty{/each}",
		"{#await p then value}{value}{:catch error}{error}{/await}",
		"{@const x = 1}",
		"<script>let x=1</script><style>p {color:red}</style>",
	] {
		assert!(
			host::parse(source, &plan, Options::default()).1.is_ok(),
			"{source}: {:?}",
			host::parse(source, &plan, Options::default()).1
		);
	}
	for source in [
		"{#each xs as{a}}{/each}",
		"{#await p then{a}}{/await}",
		"{#if x}{:else}{:else if y}{/if}",
		"<div class:foo-bar/>",
	] {
		assert!(host::parse(source, &plan, Options::default()).1.is_err(), "{source}");
	}
}

fn references(plan: &Plan, source: &str, name: &str) -> Vec<(u32, bool)> {
	let (mut ast, root) = host::parse(source, plan, Options::default());
	let root = root.unwrap_or_else(|error| panic!("{source}: {error:?}"));
	let roots = ast.add_list_from([Some(root)].into_iter());
	teasel::scopes::analyze(&mut ast, teasel::Entry::Program, roots);
	let mut refs = ast
		.scopes
		.as_ref()
		.unwrap()
		.references
		.iter()
		.filter(|r| matches!(ast.node(r.node).kind,NodeKind::Identifier{name:id} if ast.str(id)==name))
		.map(|r| (ast.node(r.node).start, r.binding.is_some()))
		.collect::<Vec<_>>();
	refs.sort();
	refs
}

#[test]
fn component_inputs_and_named_children_have_separate_regions() {
	let plan = Plan::read(include_str!("hosts/svelte/plan.json")).unwrap();
	let source = "<Comp let:x p={x}>{x}<p slot=\"named\">{x}</p><p slot=\"other\">{x}</p><p>{x}</p></Comp>";
	assert_eq!(
		references(&plan, source, "x")
			.iter()
			.map(|(_, bound)| *bound)
			.collect::<Vec<_>>(),
		[false, true, false, false, true]
	);
	let (ast, root) = host::parse(source, &plan, Options::default());
	root.unwrap();
	assert!(
		ast.host_regions
			.iter()
			.filter(|r| r.kind == teasel::host::plan::RegionKind::Fragment)
			.count() >= 5
	);
}

#[test]
fn vue_loop_sources_and_slot_inputs_stay_outside() {
	let plan = Plan::read(include_str!("hosts/vue/plan.json")).unwrap();
	assert_eq!(
		references(&plan, "<div v-for=\"item in item\" :p=\"item\">{{item}}</div>", "item")
			.iter()
			.map(|(_, b)| *b)
			.collect::<Vec<_>>(),
		[false, true, true]
	);
	assert_eq!(
		references(&plan, "<Comp v-slot=\"{x}\" :p=\"x\">{{x}}</Comp>", "x")
			.iter()
			.map(|(_, b)| *b)
			.collect::<Vec<_>>(),
		[false, true]
	);
}

#[test]
fn pattern_defaults_use_coverage_and_aliases_are_visited_once() {
	let plan = Plan::read(include_str!("hosts/svelte/plan.json")).unwrap();
	let source = "{#each xs as {x = outside}}{x}{outside}{/each}";
	assert_eq!(references(&plan, source, "x").len(), 1);
	assert_eq!(
		references(&plan, source, "outside")
			.iter()
			.map(|(_, b)| *b)
			.collect::<Vec<_>>(),
		[false, false]
	);
}

#[test]
fn native_values_preserve_node_handles() {
	let plan = document(
		r#"{"type":"Root","fields":{"expression":"null","nodeType":"null","name":"null"},"form":{"op":"seq","items":[
        {"op":"read","reader":{"kind":"token","text":"x","word":true,"gap":"none"}},
        {"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"expression"},
        {"op":"read","reader":{"kind":"token","text":";","word":false,"gap":"none"}},
        {"op":"emit","into":"nodeType","value":{"op":"get","base":"record","path":["expression","type"]}},
        {"op":"emit","into":"name","value":{"op":"get","base":"record","path":["expression","right","name"]}}
    ]}}"#,
	);
	let (ast, root) = host::parse("x a+b;", &plan, Options::default());
	let Some(Value::Nodes(children)) = field(&ast, root.unwrap(), "children") else {
		panic!()
	};
	let node = ast.nth(children, 0).unwrap();
	let Some(Value::Str(name)) = field(&ast, node, "name") else {
		panic!()
	};
	assert_eq!(ast.str(name), "b");
	let Some(Value::Str(ty)) = field(&ast, node, "nodeType") else {
		panic!()
	};
	assert_eq!(ast.str(ty), "BinaryExpression");
}

#[test]
fn unrelated_region_overlap_is_rejected() {
	let plan = document(
		r#"{"type":"Root","fields":{"value":"null"},"form":{"op":"seq","items":[
        {"op":"read","reader":{"kind":"token","text":"x","word":true,"gap":"none"}},
        {"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"value"}
    ]},"regions":[
        {"id":"one","kind":"block","parent":{"op":"get","base":"incoming","path":[]},"covers":{"op":"get","base":"record","path":["value"]}},
        {"id":"two","kind":"block","parent":{"op":"get","base":"incoming","path":[]},"covers":{"op":"get","base":"record","path":["value"]}}
    ]}"#,
	);
	let (mut ast, root) = host::parse("x value", &plan, Options::default());
	let roots = ast.add_list(&[Some(root.unwrap())]);
	teasel::scopes::analyze(&mut ast, teasel::Entry::Program, roots);
	assert_eq!(ast.scopes.as_ref().unwrap().errors.len(), 1);
	assert!(
		ast.scopes.as_ref().unwrap().errors[0]
			.message
			.contains("overlapping regions")
	);
}

#[test]
fn constructed_native_nodes_keep_native_traversal() {
	let plan = document(
		r#"{"type":"Root","fields":{"value":"null"},"form":{"op":"emit","into":"value","value":{"op":"construct","shape":"record","type":"js.BinaryExpression","span":{"op":"constant","value":null},"fields":{
        "operator":{"op":"constant","value":"+"},
        "left":{"op":"construct","shape":"record","type":"js.Identifier","span":{"op":"constant","value":null},"fields":{"name":{"op":"constant","value":"outside"}}},
        "right":{"op":"construct","shape":"record","type":"js.Literal","span":{"op":"constant","value":null},"fields":{"value":{"op":"constant","value":2}}}
    }}}}"#,
	);
	let (mut ast, root) = host::parse("", &plan, Options::default());
	let root = root.unwrap();
	let Some(Value::Node(value)) = field(&ast, root, "value") else {
		panic!()
	};
	let NodeKind::BinaryExpression { right, .. } = ast.node(value).kind else {
		panic!()
	};
	assert!(matches!(ast.node(right).kind, NodeKind::NumberLiteral {value} if ast.numbers[value as usize] == 2.0));
	let roots = ast.add_list_from([Some(root)].into_iter());
	teasel::scopes::analyze(&mut ast, teasel::Entry::Program, roots);
	assert_eq!(ast.scopes.as_ref().unwrap().references.len(), 1);
}
