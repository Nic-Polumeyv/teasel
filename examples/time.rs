use std::time::Instant;

/// The fastest of 40 runs each, which is stable across CPU frequency changes.
fn time<E>(source: &str, parse: impl Fn(&str) -> E, json: impl Fn(&E) -> String) -> (f64, f64, usize) {
	let (mut parse_ms, mut json_ms, mut bytes) = (f64::MAX, f64::MAX, 0);
	for _ in 0..40 {
		let t = Instant::now();
		let ast = parse(source);
		parse_ms = parse_ms.min(t.elapsed().as_secs_f64() * 1000.0);
		let t = Instant::now();
		let out = json(&ast);
		json_ms = json_ms.min(t.elapsed().as_secs_f64() * 1000.0);
		bytes = out.len();
	}
	(parse_ms, json_ms, bytes)
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
