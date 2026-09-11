fn main() {
	match std::env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
		"macos" => println!("cargo:rustc-cdylib-link-arg=-Wl,-undefined,dynamic_lookup"),
		"linux" => println!("cargo:rustc-link-arg=-Wl,-z,nodelete"),
		_ => {}
	}
}
