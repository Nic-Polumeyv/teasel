use teasel::host::plan::{Form, Js, Plan, Reader};

#[test]
fn checked_in_plans() {
	for (name, text) in [
		("svelte", include_str!("../hosts/svelte.json")),
		("vue", include_str!("../hosts/vue.json")),
	] {
		Plan::read(text).unwrap_or_else(|error| panic!("{name}: {error}"));
	}
}

fn plan(rules: &str) -> String {
	format!(
		r#"{{"version":1,"document":"Document","rules":{{
        "Document":{{"type":"Root","fields":{{}},"form":{{"op":"seq","items":[]}}}},
        "Text":{{"type":"Text","fields":{{}},"form":{{"op":"seq","items":[]}}}},
        "Comment":{{"type":"Comment","fields":{{}},"form":{{"op":"seq","items":[]}}}},
        "Expression":{{"type":"Expression","fields":{{}},"form":{{"op":"seq","items":[]}}}},
        {rules}
    }},"html":{{"delimiters":["{{","}}"],"attributeInterpolations":true,"attributeComments":"none",
        "autoclose":false,"trimEnd":false,"void":[],"text":"Text","comment":"Comment","content":[],"attribute":[],
        "plainAttribute":{{"type":"Attribute","name":"name","value":"value","text":"Text","expression":"Expression"}},
        "elements":[],"directiveNames":{{"prefix":"","argument":":","modifier":"|","requireArgument":false,"dynamic":null,"unknown":"plain-attribute"}},"directives":[]
    }}}}"#
	)
}

#[test]
fn invalid_plans() {
	let cases = [
		(
			"nullable shape typo",
			r#""Each":{"type":"Each","fields":{"a":"null","b":"null"},"form":{"op":"seq","items":[{"op":"emit","into":"a","value":{"op":"choose","condition":{"op":"constant","value":true},"yes":{"op":"constant","value":null},"no":{"op":"construct","shape":"record","type":null,"fields":{"known":{"op":"constant","value":1}},"span":{"op":"constant","value":null}}}},{"op":"emit","into":"b","value":{"op":"get","base":"record","path":["a","typo"]}}]}}"#,
			"unknown field \"typo\"",
		),
		(
			"unknown output",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"emit","into":"contxt","value":{"op":"constant","value":null}}}"#,
			"field \"contxt\" is emitted but not declared",
		),
		(
			"unknown capture",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"javascript","entry":"pattern"},"into":"typo"}}"#,
			"field \"typo\" is emitted but not declared",
		),
		(
			"unknown rule",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"rule","name":"Absent"}}}"#,
			"unknown rule \"Absent\"",
		),
		(
			"unknown field",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"get","base":"record","path":["b"]}}}"#,
			"unknown field \"b\"",
		),
		(
			"unknown local slot",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"get","base":"locals","path":["b"]}}}"#,
			"unknown field \"b\"",
		),
		(
			"unknown local base",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"get","base":"$typo","path":[]}}}"#,
			"unknown local reference \"$typo\"",
		),
		(
			"iteration escapes",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"get","base":"iteration","path":["item"]}}}"#,
			"unknown local context iteration",
		),
		(
			"flatmap local escapes",
			r#""Each":{"type":"Each","fields":{"a":"null","b":"null"},"form":{"op":"seq","items":[{"op":"emit","into":"a","value":{"op":"flatMap","list":{"op":"constant","value":[1]},"as":"x","body":{"op":"construct","shape":"array","items":[{"op":"get","base":"x","path":[]}]}}},{"op":"emit","into":"b","value":{"op":"get","base":"x","path":[]}}]}}"#,
			"unknown local reference \"x\"",
		),
		(
			"reserved binding",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"flatMap","list":{"op":"constant","value":[]},"as":"record","body":{"op":"constant","value":[]}}}}"#,
			"invalid local binding name",
		),
		(
			"scalar overwrite",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"seq","items":[{"op":"emit","into":"a","value":{"op":"constant","value":1}},{"op":"emit","into":"a","value":{"op":"constant","value":2}}]}}"#,
			"written twice",
		),
		(
			"choice overwrite",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"seq","items":[{"op":"choice","alternatives":[{"op":"emit","into":"a","value":{"op":"constant","value":1}},{"op":"seq","items":[]}]},{"op":"emit","into":"a","value":{"op":"constant","value":2}}]}}"#,
			"written twice",
		),
		(
			"native overwrite",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"javascript","entry":"pattern"},"into":"a"},{"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"a"}]}}"#,
			"written twice",
		),
		(
			"duplicate locals",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"locals":["a","a"]}"#,
			"duplicate local names",
		),
		(
			"field and local collision",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"seq","items":[]},"locals":["a"]}"#,
			"both a field and a local",
		),
		(
			"undeclared iteration write",
			r#""Each":{"type":"Each","fields":{"items":"null"},"form":{"op":"repeat","body":{"op":"emit","into":"wrong","value":{"op":"constant","value":1}},"min":0,"max":1,"locals":["item"],"yield":{"op":"get","base":"iteration","path":["item"]},"into":"items"}}"#,
			"field \"wrong\" is emitted but not declared",
		),
		(
			"iteration overwrite",
			r#""Each":{"type":"Each","fields":{"items":"null"},"form":{"op":"repeat","body":{"op":"seq","items":[{"op":"emit","into":"item","value":{"op":"constant","value":1}},{"op":"emit","into":"item","value":{"op":"constant","value":2}}]},"min":0,"max":1,"locals":["item"],"yield":{"op":"get","base":"iteration","path":["item"]},"into":"items"}}"#,
			"written twice",
		),
		(
			"duplicate iteration slots",
			r#""Each":{"type":"Each","fields":{"items":"null"},"form":{"op":"repeat","body":{"op":"emit","into":"item","value":{"op":"constant","value":1}},"min":0,"max":1,"locals":["item","item"],"yield":{"op":"get","base":"iteration","path":["item"]},"into":"items"}}"#,
			"duplicate locals",
		),
		(
			"unknown repeat yield",
			r#""Each":{"type":"Each","fields":{"items":"null"},"form":{"op":"repeat","body":{"op":"emit","into":"item","value":{"op":"constant","value":1}},"min":0,"max":1,"locals":["item"],"yield":{"op":"get","base":"iteration","path":["wrong"]},"into":"items"}}"#,
			"unknown field \"wrong\"",
		),
		(
			"nullable repeat",
			r#""Each":{"type":"Each","fields":{"items":"null"},"form":{"op":"repeat","body":{"op":"emit","into":"item","value":{"op":"constant","value":1}},"min":0,"max":null,"locals":["item"],"yield":{"op":"get","base":"iteration","path":["item"]},"into":"items"}}"#,
			"repeat can loop without consuming",
		),
		(
			"nullable program repeat",
			r#""Each":{"type":"Each","fields":{"items":"null"},"form":{"op":"repeat","body":{"op":"read","reader":{"kind":"javascript","entry":"program"},"into":"item"},"min":0,"max":null,"locals":["item"],"yield":{"op":"get","base":"iteration","path":["item"]},"into":"items"}}"#,
			"repeat can loop without consuming",
		),
		(
			"bounded repeat",
			r#""Each":{"type":"Each","fields":{"items":"null"},"form":{"op":"repeat","body":{"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"item","input":{"op":"get","base":"event","path":["rawChildren"]}},"min":0,"max":null,"locals":["item"],"yield":{"op":"get","base":"iteration","path":["item"]},"into":"items"}}"#,
			"repeat can loop without consuming",
		),
		(
			"bad repeat bounds",
			r#""Each":{"type":"Each","fields":{"items":"null"},"form":{"op":"repeat","body":{"op":"emit","into":"item","value":{"op":"constant","value":1}},"min":2,"max":1,"locals":["item"],"yield":{"op":"get","base":"iteration","path":["item"]},"into":"items"}}"#,
			"repeat min exceeds max",
		),
		(
			"left recursion",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"rule","name":"Each"}}}"#,
			"nullable recursion",
		),
		(
			"mutual recursion",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"rule","name":"Other"}}},"Other":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"rule","name":"Each"}}}"#,
			"nullable recursion",
		),
		(
			"nullable prefix recursion",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[{"op":"choice","alternatives":[{"op":"read","reader":{"kind":"token","text":"x","gap":"none","word":false}},{"op":"seq","items":[]}]},{"op":"read","reader":{"kind":"rule","name":"Each"}}]}}"#,
			"nullable recursion",
		),
		(
			"bounded recursion",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"token","text":"x","gap":"none","word":false}},{"op":"read","reader":{"kind":"rule","name":"Each"},"input":{"op":"get","base":"event","path":["rawChildren"]}}]}}"#,
			"recursive bounded read",
		),
		(
			"expression declaration",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"a"},"declares":[{"patterns":{"op":"get","base":"record","path":["a"]},"into":{"op":"get","base":"incoming","path":[]},"kind":"pattern"}]}"#,
			"must select pattern, bindingIdentifier, or params reads",
		),
		(
			"reference declaration",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"read","reader":{"kind":"javascript","entry":"identifierReference"},"into":"a"},"declares":[{"patterns":{"op":"get","base":"record","path":["a"]},"into":{"op":"get","base":"incoming","path":[]},"kind":"pattern"}]}"#,
			"must select pattern, bindingIdentifier, or params reads",
		),
		(
			"forged declaration",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"constant","value":{"type":"Identifier","name":"x"}}},"declares":[{"patterns":{"op":"get","base":"record","path":["a"]},"into":{"op":"get","base":"incoming","path":[]},"kind":"pattern"}]}"#,
			"must select pattern, bindingIdentifier, or params reads",
		),
		(
			"mixed declaration",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"choice","alternatives":[{"op":"read","reader":{"kind":"javascript","entry":"pattern"},"into":"a"},{"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"a"}]},"declares":[{"patterns":{"op":"get","base":"record","path":["a"]},"into":{"op":"get","base":"incoming","path":[]},"kind":"pattern"}]}"#,
			"must select pattern, bindingIdentifier, or params reads",
		),
		(
			"aliased expression declaration",
			r#""Each":{"type":"Each","fields":{"a":"null","b":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"a"},{"op":"emit","into":"b","value":{"op":"get","base":"record","path":["a"]}}]},"declares":[{"patterns":{"op":"get","base":"record","path":["b"]},"into":{"op":"get","base":"incoming","path":[]},"kind":"pattern"}]}"#,
			"must select pattern, bindingIdentifier, or params reads",
		),
		(
			"bad declaration target",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"read","reader":{"kind":"javascript","entry":"pattern"},"into":"a"},"declares":[{"patterns":{"op":"get","base":"record","path":["a"]},"into":{"op":"constant","value":1},"kind":"pattern"}]}"#,
			"must select a region",
		),
		(
			"unknown declaration region",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"read","reader":{"kind":"javascript","entry":"pattern"},"into":"a"},"declares":[{"patterns":{"op":"get","base":"record","path":["a"]},"into":{"op":"get","base":"scopes","path":["missing"]},"kind":"pattern"}]}"#,
			"unknown field \"missing\"",
		),
		(
			"unknown parent",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"a","parent":{"op":"get","base":"scopes","path":["missing"]},"kind":"block","covers":{"op":"constant","value":[]}}]}"#,
			"unknown field \"missing\"",
		),
		(
			"self parent",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"a","parent":{"op":"get","base":"scopes","path":["a"]},"kind":"block","covers":{"op":"constant","value":[]}}]}"#,
			"region parents form a cycle",
		),
		(
			"cyclic parents",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"a","parent":{"op":"get","base":"scopes","path":["b"]},"kind":"block","covers":{"op":"constant","value":[]}},{"id":"b","parent":{"op":"get","base":"scopes","path":["a"]},"kind":"block","covers":{"op":"constant","value":[]}}]}"#,
			"region parents form a cycle",
		),
		(
			"nested parent path cycle",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"a","parent":{"op":"get","base":{"op":"get","base":"scopes","path":[]},"path":["a"]},"kind":"block","covers":{"op":"constant","value":[]}}]}"#,
			"region parents form a cycle",
		),
		(
			"aliased parent cycle",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"emit","into":"parent","value":{"op":"get","base":"scopes","path":["a"]}},"locals":["parent"],"regions":[{"id":"a","parent":{"op":"get","base":"locals","path":["parent"]},"kind":"block","covers":{"op":"constant","value":[]}}]}"#,
			"region parents form a cycle",
		),
		(
			"duplicate regions",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"a","parent":{"op":"get","base":"incoming","path":[]},"kind":"block","covers":{"op":"constant","value":[]}},{"id":"a","parent":{"op":"get","base":"incoming","path":[]},"kind":"block","covers":{"op":"constant","value":[]}}]}"#,
			"duplicate region names",
		),
		(
			"bad parent type",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"a","parent":{"op":"constant","value":"incoming"},"kind":"block","covers":{"op":"constant","value":[]}}]}"#,
			"must select a region or null",
		),
		(
			"unknown coverage field",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"a","parent":{"op":"get","base":"incoming","path":[]},"kind":"block","covers":{"op":"get","base":"record","path":["missing"]}}]}"#,
			"unknown field \"missing\"",
		),
		(
			"bad coverage type",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"constant","value":3}},"regions":[{"id":"scope","parent":{"op":"get","base":"incoming","path":[]},"kind":"block","covers":{"op":"get","base":"record","path":["a"]}}]}"#,
			"must select node roots",
		),
		(
			"unknown region iterator",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"a","parent":{"op":"get","base":"incoming","path":[]},"kind":"block","covers":{"op":"get","base":"$missing","path":[]},"each":{"list":{"op":"constant","value":[]},"as":"$item"}}]}"#,
			"unknown local reference \"$missing\"",
		),
		(
			"bad region iterator",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"a","parent":{"op":"get","base":"incoming","path":[]},"kind":"block","covers":{"op":"constant","value":[]},"each":{"list":{"op":"constant","value":1},"as":"$item"}}]}"#,
			"requires a list",
		),
		(
			"length scalar",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"length","list":{"op":"constant","value":1}}}}"#,
			"length requires a list",
		),
		(
			"length native expression",
			r#""Each":{"type":"Each","fields":{"a":"null","b":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"javascript","entry":"expression"},"into":"a"},{"op":"emit","into":"b","value":{"op":"length","list":{"op":"get","base":"record","path":["a"]}}}]}}"#,
			"length requires a list",
		),
		(
			"at scalar",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"at","list":{"op":"constant","value":"x"},"index":{"op":"constant","value":0}}}}"#,
			"at requires a list",
		),
		(
			"at index type",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"at","list":{"op":"constant","value":[]},"index":{"op":"constant","value":"x"}}}}"#,
			"at index must be a number",
		),
		(
			"flatmap scalar",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"flatMap","list":{"op":"constant","value":1},"as":"item","body":{"op":"constant","value":[]}}}}"#,
			"flatMap requires a list",
		),
		(
			"flatmap scalar yield",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"flatMap","list":{"op":"constant","value":[]},"as":"item","body":{"op":"constant","value":1}}}}"#,
			"flatMap body must be a list",
		),
		(
			"nonboolean choose",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"choose","condition":{"op":"constant","value":"yes"},"yes":{"op":"constant","value":1},"no":{"op":"constant","value":2}}}}"#,
			"choose condition must be boolean",
		),
		(
			"nonnumeric comparison",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"compare","relation":"less","left":{"op":"constant","value":true},"right":{"op":"constant","value":0}}}}"#,
			"less requires numbers",
		),
		(
			"nonboolean test",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"test","value":{"op":"constant","value":1}}}}"#,
			"test value must be boolean",
		),
		(
			"invalid record span",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"construct","shape":"record","type":null,"fields":{},"span":{"op":"constant","value":0}}}}"#,
			"construct span must be a span or null",
		),
		(
			"get off scalar",
			r#""Each":{"type":"Each","fields":{"a":"null","b":"null"},"form":{"op":"seq","items":[{"op":"emit","into":"a","value":{"op":"constant","value":1}},{"op":"emit","into":"b","value":{"op":"get","base":"record","path":["a","missing"]}}]}}"#,
			"unknown field \"missing\"",
		),
		(
			"known record typo",
			r#""Each":{"type":"Each","fields":{"a":"null","b":"null"},"form":{"op":"seq","items":[{"op":"emit","into":"a","value":{"op":"construct","shape":"record","type":null,"fields":{"x":{"op":"constant","value":1}},"span":{"op":"constant","value":null}}},{"op":"emit","into":"b","value":{"op":"get","base":"record","path":["a","missing"]}}]}}"#,
			"unknown field \"missing\"",
		),
		(
			"called shape typo",
			r#""Each":{"type":"Each","fields":{"a":"null","b":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"rule","name":"Other"},"into":"a"},{"op":"emit","into":"b","value":{"op":"get","base":"record","path":["a","missing"]}}]}},"Other":{"type":"Each","fields":{"x":"null"},"form":{"op":"emit","into":"x","value":{"op":"constant","value":1}}}"#,
			"unknown field \"missing\"",
		),
		(
			"native identifier typo",
			r#""Each":{"type":"Each","fields":{"a":"null","b":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"javascript","entry":"bindingIdentifier"},"into":"a"},{"op":"emit","into":"b","value":{"op":"get","base":"record","path":["a","missing"]}}]}}"#,
			"unknown field \"missing\"",
		),
		(
			"native program typo",
			r#""Each":{"type":"Each","fields":{"a":"null","b":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"javascript","entry":"program"},"into":"a"},{"op":"emit","into":"b","value":{"op":"get","base":"record","path":["a","missing"]}}]}}"#,
			"unknown field \"missing\"",
		),
		(
			"html-single lacks input",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"html-single","entry":"expression"}}}"#,
			"html-single input must be an attribute value",
		),
		(
			"html-single span input",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"html-single","entry":"expression"},"input":{"op":"get","base":"event","path":["rawChildren"]}}}"#,
			"html-single input must be an attribute value",
		),
		(
			"javascript interpolation input",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"javascript","entry":"expression"},"input":{"op":"get","base":"event","path":["value"]}}}"#,
			"javascript input must be a source span",
		),
		(
			"javascript scalar input",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"javascript","entry":"expression"},"input":{"op":"constant","value":"code"}}}"#,
			"javascript input must be a source span",
		),
		(
			"css lacks input",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"css-stylesheet"}}}"#,
			"css-stylesheet input must be a source span",
		),
		(
			"rule scalar input",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"rule","name":"Expression"},"input":{"op":"constant","value":1}}}"#,
			"rule input must be a source span",
		),
		(
			"token bounded input",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"token","text":"x","gap":"none","word":false},"input":{"op":"get","base":"event","path":["rawChildren"]}}}"#,
			"token/space reader does not accept input",
		),
		(
			"invalid boundary entry",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"javascript","entry":"pattern","boundary":"last-shared-word"}}}"#,
			"last-shared-word requires an unbounded expression read",
		),
		(
			"unknown operation",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"maybe"}}"#,
			"unknown form operation \"maybe\"",
		),
		(
			"unknown value operation",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"map","list":{"op":"constant","value":[]}}}}"#,
			"unknown value operation \"map\"",
		),
		(
			"unknown reader",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"js"}}}"#,
			"unknown reader \"js\"",
		),
		(
			"wrong absence policy",
			r#""Each":{"type":"Each","fields":{"x":"undefined"},"form":{"op":"seq","items":[]}}"#,
			"unknown Absence \"undefined\"",
		),
		(
			"unknown form field",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[],"extra":1}}"#,
			"unknown field \"extra\"",
		),
		(
			"empty choice",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"choice","alternatives":[]}}"#,
			"choice requires an alternative",
		),
		(
			"empty token",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"token","text":"","gap":"none","word":false}}}"#,
			"token text must not be empty",
		),
		(
			"missing comparison operand",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"compare","relation":"equal","left":{"op":"constant","value":1}}}}"#,
			"right",
		),
		(
			"unexpected present operand",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"compare","relation":"present","left":{"op":"constant","value":1},"right":{"op":"constant","value":1}}}}"#,
			"right",
		),
		(
			"multiple stop conditions",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"html-children","mode":"normal","stop":{"matchingElement":true,"documentEnd":true}}}}"#,
			"stop must have exactly one condition",
		),
		(
			"false stop condition",
			r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"html-children","mode":"normal","stop":{"matchingElement":false}}}}"#,
			"matchingElement must be true",
		),
		(
			"negative path index",
			r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"emit","into":"a","value":{"op":"get","base":{"op":"constant","value":[]},"path":[-1]}}}"#,
			"expected nonnegative integer",
		),
	];
	for (label, rules, expected) in cases {
		let error = Plan::read(&plan(rules)).unwrap_err();
		assert!(
			error.contains("rule Each") && error.contains(expected),
			"{label}: expected {expected:?}, got {error}"
		);
	}
}

fn expression_program_choice(form: &Form) -> Option<bool> {
	match form {
		Form::Choice { alternatives, disjoint } => {
			if matches!(
				alternatives.as_slice(),
				[
					Form::Read {
						reader: Reader::Javascript {
							entry: Js::Expression,
							..
						},
						..
					},
					Form::Read {
						reader: Reader::Javascript { entry: Js::Program, .. },
						..
					}
				]
			) {
				return Some(*disjoint);
			}
			alternatives.iter().find_map(expression_program_choice)
		}
		Form::Seq(items) => items.iter().find_map(expression_program_choice),
		Form::Repeat { body, .. } => expression_program_choice(body),
		_ => None,
	}
}

#[test]
fn header_choices() {
	let text = include_str!("../hosts/svelte.json");
	let header = r#""Headers": {"type":"Header","fields":{},"form":{"op":"choice","alternatives":[
        {"op":"read","reader":{"kind":"rule","name":"If"}},
        {"op":"read","reader":{"kind":"rule","name":"Each"}},
        {"op":"read","reader":{"kind":"rule","name":"Await"}},
        {"op":"read","reader":{"kind":"rule","name":"Key"}},
        {"op":"read","reader":{"kind":"rule","name":"Snippet"}}
    ]}},"#;
	let text = text.replacen("\"rules\": {", &format!("\"rules\": {{{header}"), 1);
	let plan = Plan::read(&text).unwrap();
	let headers = plan.rules.iter().find(|r| r.name.as_ref() == "Headers").unwrap();
	let Form::Choice { alternatives, disjoint } = &headers.form else {
		panic!()
	};
	assert!(disjoint);
	for (form, expected) in alternatives.iter().zip(["If", "Each", "Await", "Key", "Snippet"]) {
		let Form::Read {
			reader: Reader::Rule(index),
			..
		} = form
		else {
			panic!()
		};
		assert_eq!(plan.rules[*index].name.as_ref(), expected);
	}
	let vue = Plan::read(include_str!("../hosts/vue.json")).unwrap();
	let on = vue.rules.iter().find(|r| r.name.as_ref() == "On").unwrap();
	assert_eq!(expression_program_choice(&on.form), Some(false));
}

#[test]
fn invalid_dispatch_and_channels() {
	let base = plan(r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]}}"#);
	let cases = [
		(
			"document",
			"\"document\":\"Document\"",
			"\"document\":\"Absent\"",
			"unknown rule \"Absent\"",
		),
		(
			"text",
			"\"text\":\"Text\"",
			"\"text\":\"Absent\"",
			"unknown rule \"Absent\"",
		),
		(
			"comment",
			"\"comment\":\"Comment\"",
			"\"comment\":\"Absent\"",
			"unknown rule \"Absent\"",
		),
		(
			"plain expression",
			"\"expression\":\"Expression\"",
			"\"expression\":\"Absent\"",
			"unknown rule \"Absent\"",
		),
		(
			"content",
			"\"content\":[]",
			"\"content\":[{\"prefix\":\"x\",\"rule\":\"Absent\"}]",
			"unknown rule \"Absent\"",
		),
		(
			"attribute",
			"\"attribute\":[]",
			"\"attribute\":[{\"prefix\":\"x\",\"rule\":\"Absent\"}]",
			"unknown rule \"Absent\"",
		),
		(
			"directive",
			"\"directives\":[]",
			"\"directives\":[{\"name\":\"x\",\"rule\":\"Absent\"}]",
			"unknown rule \"Absent\"",
		),
		(
			"element",
			"\"elements\":[]",
			"\"elements\":[{\"when\":{\"op\":\"constant\",\"value\":true},\"rule\":\"Absent\"}]",
			"unknown rule \"Absent\"",
		),
		(
			"dispatch predicate",
			"\"elements\":[]",
			"\"elements\":[{\"when\":{\"op\":\"constant\",\"value\":1},\"rule\":\"Each\"}]",
			"must be boolean",
		),
		(
			"dispatch lexical typo",
			"\"elements\":[]",
			"\"elements\":[{\"when\":{\"op\":\"get\",\"base\":\"event\",\"path\":[\"nameFacts\",\"misspelled\"]},\"rule\":\"Each\"}]",
			"unknown field \"misspelled\"",
		),
		("version", "\"version\":1", "\"version\":2", "unsupported version"),
		(
			"delimiter",
			"\"delimiters\":[\"{\",\"}\"]",
			"\"delimiters\":[\"\",\"}\"]",
			"delimiters must not be empty",
		),
	];
	for (label, from, to, expected) in cases {
		assert!(base.contains(from), "{label}");
		let error = Plan::read(&base.replacen(from, to, 1)).unwrap_err();
		assert!(error.contains(expected), "{label}: {error}");
	}
	let cases = [
		(
			r#"{"kind":"html-attributes","mode":"normal"}"#,
			"html-attributes reader requires an element channel",
		),
		(
			r#"{"kind":"html-children","mode":"normal","stop":{"matchingElement":true}}"#,
			"html-children stop/input does not match its channel",
		),
		(
			r#"{"kind":"html-attribute-parts"}"#,
			"html-attribute-parts reader requires an attribute channel",
		),
	];
	for (reader, expected) in cases {
		let rules = format!(r#""Each":{{"type":"Each","fields":{{}},"form":{{"op":"read","reader":{reader}}}}}"#);
		let text = plan(&rules).replace("\"content\":[]", r#""content":[{"prefix":"x","rule":"Each"}]"#);
		let error = Plan::read(&text).unwrap_err();
		assert!(error.contains("rule Each") && error.contains(expected), "{error}");
	}
	let text = plan(r#""Each":{"type":"Each","fields":{},"form":{"op":"read","reader":{"kind":"rule","name":"Other"}}},"Other":{"type":"Other","fields":{},"form":{"op":"read","reader":{"kind":"html-attributes","mode":"normal"}}}"#)
        .replace("\"content\":[]", r#""content":[{"prefix":"x","rule":"Each"}]"#);
	let error = Plan::read(&text).unwrap_err();
	assert!(
		error.contains("rule Other") && error.contains("requires an element channel"),
		"{error}"
	);
}

#[test]
fn html_dispatch_makes_progress() {
	let cases = [
		(
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]}}"#,
			"dispatch can succeed without consuming input",
		),
		(
			r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"html-children","mode":"normal","stop":{"prefixes":["end"]}}},{"op":"read","reader":{"kind":"token","text":"x","gap":"none","word":true}}]}}"#,
			"nullable recursion",
		),
	];
	for (rules, expected) in cases {
		let text = plan(rules).replace("\"content\":[]", r#""content":[{"prefix":"x","rule":"Each"}]"#);
		let error = Plan::read(&text).unwrap_err();
		assert!(error.contains("rule Each") && error.contains(expected), "{error}");
	}
	let text = plan(r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"html-children","mode":"normal","stop":{"prefixes":["x"]}}},{"op":"read","reader":{"kind":"token","text":"x","gap":"none","word":true}}]}}"#)
        .replace("\"content\":[]", r#""content":[{"prefix":"x","rule":"Each"}]"#);
	Plan::read(&text).unwrap();
}

#[test]
fn valid_capture_and_region_paths() {
	let cases = [
		r#""Each":{"type":"Each","fields":{"a":"null","missing":"omit"},"form":{"op":"seq","items":[{"op":"emit","into":"a","value":{"op":"constant","value":1}},{"op":"emit","into":"a","value":{"op":"get","base":"record","path":["missing"]}}]}}"#,
		r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"choice","alternatives":[{"op":"seq","items":[{"op":"emit","into":"a","value":{"op":"constant","value":1}},{"op":"emit","into":"a","value":{"op":"constant","value":2}},{"op":"read","reader":{"kind":"test","value":{"op":"constant","value":false}}}]},{"op":"emit","into":"a","value":{"op":"constant","value":3}}]}}"#,
		r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"choice","alternatives":[{"op":"emit","into":"a","value":{"op":"constant","value":1}},{"op":"emit","into":"a","value":{"op":"constant","value":2}}]}}"#,
		r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"javascript","entry":"pattern"},"into":"hidden"},{"op":"emit","into":"a","value":{"op":"get","base":"locals","path":["hidden"]}}]},"locals":["hidden"],"declares":[{"patterns":{"op":"get","base":"record","path":["a"]},"into":{"op":"get","base":"incoming","path":[]},"kind":"pattern"}]}"#,
		r#""Each":{"type":"Each","fields":{"a":"null","size":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"javascript","entry":"params"},"into":"a"},{"op":"emit","into":"size","value":{"op":"length","list":{"op":"get","base":"record","path":["a"]}}}]},"declares":[{"patterns":{"op":"get","base":"record","path":["a"]},"into":{"op":"get","base":"incoming","path":[]},"kind":"param"}]}"#,
		r#""Each":{"type":"Each","fields":{"items":"omit"},"form":{"op":"repeat","body":{"op":"emit","into":"item","value":{"op":"constant","value":1}},"min":0,"max":1,"locals":["item"],"yield":{"op":"get","base":"iteration","path":["item"]},"into":"items"}}"#,
		r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"token","text":"x","gap":"none","word":false}},{"op":"choice","alternatives":[{"op":"read","reader":{"kind":"rule","name":"Each"},"into":"a"},{"op":"seq","items":[]}]}]}}"#,
		r#""Each":{"type":"Each","fields":{},"form":{"op":"seq","items":[]},"regions":[{"id":"child","parent":{"op":"get","base":"scopes","path":["parent"]},"kind":"block","covers":{"op":"constant","value":[]}},{"id":"parent","parent":{"op":"get","base":"incoming","path":[]},"kind":"block","covers":{"op":"constant","value":[]},"when":{"op":"constant","value":false}}]}"#,
		r#""Each":{"type":"Each","fields":{"a":"null"},"form":{"op":"seq","items":[{"op":"read","reader":{"kind":"rule","name":"Other"},"into":"a"}]},"declares":[{"patterns":{"op":"get","base":"record","path":["a","pattern"]},"into":{"op":"get","base":"incoming","path":[]},"kind":"pattern"}]},"Other":{"type":"Pattern","fields":{"pattern":"null"},"form":{"op":"read","reader":{"kind":"javascript","entry":"pattern"},"into":"pattern"}}"#,
	];
	for rules in cases {
		Plan::read(&plan(rules)).unwrap_or_else(|error| panic!("{error}"));
	}
}

#[test]
fn json_syntax_and_locations() {
	use teasel::host::plan::{Json, Value};
	let text = plan(
		r#""Each":{"type":"\u0045ach","fields":{"value":"null"},"form":{"op":"emit","into":"value","value":{"op":"constant","value":[null,true,false,-12.5e+30,"\"\\\/\b\f\n\r\t\uD83D\uDE00é",{"key":1E400}]}}}"#,
	);
	let parsed = Plan::read(&text).unwrap();
	let rule = parsed.rules.iter().find(|r| r.name.as_ref() == "Each").unwrap();
	assert_eq!(rule.node_type.as_ref(), "Each");
	let Form::Emit {
		value: Value::Constant(Json::Array(values)),
		..
	} = &rule.form
	else {
		panic!()
	};
	assert_eq!(values[3], Json::Number("-12.5e+30".into()));
	assert_eq!(values[4], Json::String("\"\\/\u{8}\u{c}\n\r\t😀é".into()));
	let invalid = [
		("null", "expected object"),
		("{\n  !", "2:3:"),
		("{\"version\":01}", "expected '}'"),
		("{\"version\":1.}", "expected digit"),
		("{\"version\":1e+}", "expected digit"),
		("{\"version\":-}", "invalid number"),
		("{\"version\":true,}", "expected '\"'"),
		("{\"version\":[1,]}", "expected JSON value"),
		("{\"version\":NaN}", "expected JSON value"),
		("{\"version\":tru}", "expected 'e'"),
		("{\"version\":\"\\q\"}", "invalid string escape"),
		("{\"version\":\"\\uD800\\u0000\"}", "invalid unicode surrogate pair"),
		("{\"version\":\"\\uDC00\"}", "unpaired unicode surrogate"),
		("{\"version\":\"\\uXXXX\"}", "invalid unicode escape"),
		("{\"version\":\"line\nbreak\"}", "unescaped control character"),
		("{\"version\":\"", "unterminated string"),
		("{\"version\":\"\\", "unterminated escape"),
	];
	for (text, expected) in invalid {
		let error = Plan::read(text).unwrap_err();
		assert!(error.contains(expected), "{text:?}: {error}");
		assert!(error.split(':').next().unwrap().parse::<usize>().is_ok(), "{error}");
	}
	let error = Plan::read(&(text.clone() + " false")).unwrap_err();
	assert!(error.contains("trailing JSON input"), "{error}");
	let duplicate = plan(r#""Each":{"type":"Each","fields":{"a":"null","a":"omit"},"form":{"op":"seq","items":[]}}"#);
	let error = Plan::read(&duplicate).unwrap_err();
	assert!(
		error.contains("rule Each: field \"a\": duplicate field")
			&& error.contains("absence policies must be consistent"),
		"{error}"
	);
	let deep = format!("{}null{}", "[".repeat(130), "]".repeat(130));
	assert!(Plan::read(&deep).unwrap_err().contains("JSON nesting exceeds 128"));
	for (end, _) in text.char_indices() {
		assert!(Plan::read(&text[..end]).is_err(), "accepted prefix at {end}");
	}
}

#[test]
fn first_tokens_respect_gaps_and_boundaries() {
	let token = |text: &str, gap: &str, word: bool| {
		format!(r#"{{"op":"read","reader":{{"kind":"token","text":"{text}","gap":"{gap}","word":{word}}}}}"#)
	};
	let cases = [
		(
			format!(
				r#"{{"op":"seq","items":[{},{{"op":"seq","items":[{}]}}]}}"#,
				token("{", "space*", false),
				token("#if", "space*", true)
			),
			token("{ #if", "space*", true),
			false,
		),
		(token("if", "none", true), token("each", "none", true), true),
		(token("if", "none", true), token("iffy", "none", true), true),
		(token("if", "none", false), token("iffy", "none", true), false),
		(token("if", "none", true), token("if", "space*", true), false),
		(token(" if", "none", true), token("if", "space*", true), false),
		(
			format!(
				r#"{{"op":"seq","items":[{},{}]}}"#,
				token("{", "space*", false),
				token("#if", "none", true)
			),
			token("{#each", "space*", true),
			true,
		),
		(
			format!(
				r#"{{"op":"seq","items":[{},{}]}}"#,
				token("{", "space*", false),
				token("#if", "space*", true)
			),
			token("{ #if", "space*", true),
			false,
		),
		(token("é", "none", false), token("ê", "none", false), true),
		(token("x", "none", true), r#"{"op":"seq","items":[]}"#.into(), false),
		(
			token("x", "none", true),
			r#"{"op":"read","reader":{"kind":"javascript","entry":"expression"}}"#.into(),
			false,
		),
	];
	for (left, right, expected) in cases {
		let rules = format!(
			r#""Each":{{"type":"Each","fields":{{}},"form":{{"op":"choice","alternatives":[{left},{right}]}}}}"#
		);
		let plan = Plan::read(&plan(&rules)).unwrap();
		let each = plan.rules.iter().find(|r| r.name.as_ref() == "Each").unwrap();
		let Form::Choice { disjoint, .. } = &each.form else {
			panic!()
		};
		assert_eq!(*disjoint, expected, "{left} / {right}");
	}
}
