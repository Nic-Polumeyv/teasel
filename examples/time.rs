use std::time::Instant;

fn time<E>(source: &str, parse: impl Fn(&str) -> E, json: impl Fn(&E) -> String) -> (f64, f64, usize) {
	let (mut parse_ms, mut json_ms, mut bytes) = (0.0, 0.0, 0);
	for i in 0..23 {
		let t = Instant::now();
		let ast = parse(source);
		let parsed = t.elapsed().as_secs_f64() * 1000.0;
		let t = Instant::now();
		let out = json(&ast);
		let serialized = t.elapsed().as_secs_f64() * 1000.0;
		if i >= 3 {
			parse_ms += parsed;
			json_ms += serialized;
		}
		bytes = out.len();
	}
	(parse_ms / 20.0, json_ms / 20.0, bytes)
}

fn main() {
	let path = std::env::args().nth(1).unwrap();
	let source = std::fs::read_to_string(&path).unwrap();
	let options = teasel::Options {
		module: true,
		..Default::default()
	};
	let (parse_ms, json_ms, bytes) = if path.ends_with(".ts") {
		time(
			&source,
			|s| teasel::typescript::parse(s, options).unwrap(),
			|ast| teasel::estree::to_json(ast, ast.last(), &source, true),
		)
	} else {
		time(
			&source,
			|s| teasel::parse(s, options).unwrap(),
			|ast| teasel::estree::to_json(ast, ast.last(), &source, true),
		)
	};
	println!(
		"{} bytes source, parse {parse_ms:.2} ms, to_json {json_ms:.2} ms ({} KB)",
		source.len(),
		bytes / 1024
	);
}
