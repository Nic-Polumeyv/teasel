use teasel::Options;
use teasel::ast::{Ast, NodeId, NodeKind, Value};
use teasel::host::{executor, plan::Plan};

fn field(ast: &Ast, node: NodeId, name: &str) -> Option<Value> {
	let NodeKind::Host(index) = ast.node(node).kind else {
		return None;
	};
	let host = ast.hosts[index as usize];
	ast.host_fields[host.fields.0 as usize..(host.fields.0 + host.fields.1) as usize]
		.iter()
		.find(|(key, _)| ast.str(*key) == name)
		.map(|(_, value)| *value)
}

#[test]
fn native_entries_and_html() {
	let plan = Plan::read(include_str!("hosts/vue/plan.json")).unwrap();
	let (ast, root) = executor::parse("<p title=\"a&amp;b\">hello {{name}}</p>", &plan, Options::default());
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
        "autoclose":false,"trimEnd":false,"void":[],"text":"Text","comment":"Comment","content":[],"attribute":[],
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
	let (ast, root) = executor::parse("", &plan, Options::default());
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
fn repeat_uses_fresh_slots() {
	let plan = document(
		r#"{"type":"Root","fields":{"items":"null"},"form":{"op":"repeat","min":2,"max":2,"locals":["item"],"into":"items","body":{"op":"read","reader":{"kind":"token","text":"x","word":true,"gap":"space*"},"into":"item"},"yield":{"op":"get","base":"iteration","path":["item","text"]}}}"#,
	);
	let (ast, root) = executor::parse("x x", &plan, Options::default());
	let Some(Value::Nodes(children)) = field(&ast, root.unwrap(), "children") else {
		panic!()
	};
	let Some(Value::Strs(_, len)) = field(&ast, ast.nth(children, 0).unwrap(), "items") else {
		panic!()
	};
	assert_eq!(len, 2);
}
