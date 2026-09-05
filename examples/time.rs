use std::time::Instant;

fn main() {
	let path = std::env::args().nth(1).unwrap();
	let typescript = path.ends_with(".ts");
	let source = std::fs::read_to_string(&path).unwrap();
	let options = teasel::Options {
		module: true,
		..Default::default()
	};
	let n = 20;
	let mut parse_ms = 0.0;
	let mut json_ms = 0.0;
	let mut bytes = 0;
	for _ in 0..n {
		let t = Instant::now();
		if typescript {
			let ast = teasel::typescript::parse(&source, options).unwrap();
			parse_ms += t.elapsed().as_secs_f64() * 1000.0;
			let t = Instant::now();
			let json = teasel::estree::to_json(&ast, ast.last(), &source);
			json_ms += t.elapsed().as_secs_f64() * 1000.0;
			bytes = json.len();
		} else {
			let ast = teasel::parse(&source, options).unwrap();
			parse_ms += t.elapsed().as_secs_f64() * 1000.0;
			let t = Instant::now();
			let json = teasel::estree::to_json(&ast, ast.last(), &source);
			json_ms += t.elapsed().as_secs_f64() * 1000.0;
			bytes = json.len();
		}
	}
	println!(
		"{} bytes source, parse {:.2} ms, to_json {:.2} ms ({} KB)",
		source.len(),
		parse_ms / n as f64,
		json_ms / n as f64,
		bytes / 1024
	);
}
