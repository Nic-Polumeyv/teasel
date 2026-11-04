use std::fmt::Write;

/// JSON text.
pub(crate) struct Json {
	out: String,
	/// Whether the next entry of the open container is its first.
	first: bool,
	/// The open containers, `true` for a list.
	stack: Vec<bool>,
}

impl Default for Json {
	fn default() -> Self {
		Json {
			out: String::new(),
			first: true,
			stack: Vec::new(),
		}
	}
}

impl Json {
	pub(crate) fn finish(self) -> String {
		self.out
	}

	fn sep(&mut self) {
		if !self.first {
			self.out.push(',');
		}
		self.first = false;
	}

	fn open(&mut self, list: bool) {
		self.sep();
		self.out.push(if list { '[' } else { '{' });
		self.stack.push(list);
		self.first = true;
	}

	pub(crate) fn ints(&mut self, values: &[u32]) {
		self.list();
		for &value in values {
			self.int(value);
		}
		self.end();
	}

	pub(crate) fn begin(&mut self, ty: &str) {
		self.open(false);
		self.key("type");
		self.text(ty);
	}

	pub(crate) fn object(&mut self) {
		self.open(false);
	}

	pub(crate) fn list(&mut self) {
		self.open(true);
	}

	pub(crate) fn end(&mut self) {
		self.out.push(if self.stack.pop() == Some(true) { ']' } else { '}' });
		self.first = false;
	}

	pub(crate) fn key(&mut self, key: &str) {
		self.sep();
		write_json_string(&mut self.out, key);
		self.out.push(':');
		self.first = true;
	}

	pub(crate) fn int(&mut self, value: u32) {
		self.sep();
		write!(self.out, "{value}").unwrap();
	}

	pub(crate) fn float(&mut self, value: f64) {
		self.sep();
		write_number(&mut self.out, value);
	}

	pub(crate) fn bool(&mut self, value: bool) {
		self.sep();
		self.out.push_str(if value { "true" } else { "false" });
	}

	pub(crate) fn null(&mut self) {
		self.sep();
		self.out.push_str("null");
	}

	pub(crate) fn text(&mut self, value: &str) {
		self.sep();
		write_json_string(&mut self.out, value);
	}

	/// A string with lone surrogates at `marks`, as the interner keeps them: each written as
	/// the escape JSON has for it.
	pub(crate) fn marked(&mut self, value: &str, marks: &[crate::interner::Surrogate]) {
		if marks.is_empty() {
			return self.text(value);
		}
		self.sep();
		self.out.push('"');
		let mut marks = marks.iter();
		let mut next = marks.next();
		let mut units = 0u32;
		let mut from = 0usize;
		for (i, c) in value.char_indices() {
			if let Some(mark) = next
				&& mark.at == units
			{
				escape_json(&mut self.out, &value[from..i]);
				write!(self.out, "\\u{:04x}", mark.unit).unwrap();
				from = i + c.len_utf8();
				next = marks.next();
			}
			units += c.len_utf16() as u32;
		}
		escape_json(&mut self.out, &value[from..]);
		self.out.push('"');
	}
}

fn write_number(out: &mut String, value: f64) {
	if !value.is_finite() {
		out.push_str("null");
		return;
	}
	if value == 0.0 {
		out.push('0');
		return;
	}
	let formatted = format!("{value:e}");
	let (mantissa, exponent) = formatted.split_once('e').unwrap();
	let (sign, mantissa) = mantissa.strip_prefix('-').map_or(("", mantissa), |m| ("-", m));
	let mut digits: Vec<u8> = mantissa.bytes().filter(|b| *b != b'.').collect();
	even_on_tie(value.abs(), &mut digits);
	let digits = std::str::from_utf8(&digits).unwrap();
	let k = digits.len() as i32;
	let n = exponent.parse::<i32>().unwrap() + 1;
	out.push_str(sign);
	if k <= n && n <= 21 {
		out.push_str(digits);
		out.extend(std::iter::repeat_n('0', (n - k) as usize));
	} else if 0 < n && n <= 21 {
		out.push_str(&digits[..n as usize]);
		out.push('.');
		out.push_str(&digits[n as usize..]);
	} else if -6 < n && n <= 0 {
		out.push_str("0.");
		out.extend(std::iter::repeat_n('0', (-n) as usize));
		out.push_str(digits);
	} else {
		out.push_str(&digits[..1]);
		if k > 1 {
			out.push('.');
			out.push_str(&digits[1..]);
		}
		out.push('e');
		out.push(if n > 1 { '+' } else { '-' });
		write!(out, "{}", (n - 1).unsigned_abs()).unwrap();
	}
}

/// Rust rounds the shortest digits away from zero on an exact tie; JavaScript takes the even ones.
fn even_on_tie(value: f64, digits: &mut [u8]) {
	let k = digits.len();
	// two shortest forms sit at the same distance only at the edge of what a double resolves
	if k < 16 || digits[k - 1].is_multiple_of(2) {
		return;
	}
	let exact = format!("{value:.*e}", 1100);
	let all: Vec<u8> = exact
		.split_once('e')
		.unwrap()
		.0
		.bytes()
		.filter(|b| *b != b'.')
		.collect();
	if all[k] != b'5' || all[k + 1..].iter().any(|&b| b != b'0') {
		return;
	}
	let mut lower = digits.to_vec();
	lower[k - 1] -= 1;
	let lower = std::str::from_utf8(&lower).unwrap();
	let exponent = exact.split_once('e').unwrap().1;
	if let Ok(back) = format!("{}.{}e{exponent}", &lower[..1], &lower[1..]).parse::<f64>()
		&& back == value
	{
		digits[k - 1] -= 1;
	}
}

pub(crate) fn write_json_string(out: &mut String, s: &str) {
	out.push('"');
	escape_json(out, s);
	out.push('"');
}

fn escape_json(out: &mut String, s: &str) {
	let mut from = 0;
	for (i, b) in s.bytes().enumerate() {
		if b >= 0x20 && b != b'"' && b != b'\\' {
			continue;
		}
		out.push_str(&s[from..i]);
		match b {
			b'"' => out.push_str("\\\""),
			b'\\' => out.push_str("\\\\"),
			b'\n' => out.push_str("\\n"),
			b'\r' => out.push_str("\\r"),
			b'\t' => out.push_str("\\t"),
			8 => out.push_str("\\b"),
			12 => out.push_str("\\f"),
			_ => write!(out, "\\u{b:04x}").unwrap(),
		}
		from = i + 1;
	}
	out.push_str(&s[from..]);
}

#[cfg(test)]
mod tests {
	#[test]
	#[expect(clippy::excessive_precision)]
	fn numbers_as_javascript_writes_them() {
		for (value, text) in [
			(1e-14, "1e-14"),
			(1e-7, "1e-7"),
			(0.000001, "0.000001"),
			(0.1, "0.1"),
			(1.5, "1.5"),
			(123.456, "123.456"),
			(100.0, "100"),
			(1e20, "100000000000000000000"),
			(1e21, "1e+21"),
			(1.5e300, "1.5e+300"),
			(5e-324, "5e-324"),
			(-0.0, "0"),
			(-2.5e-8, "-2.5e-8"),
			(132405809496.45312, "132405809496.45312"),
			(0.3, "0.3"),
			(9007199254740993.0, "9007199254740992"),
			(1658206780088562.25, "1658206780088562.2"),
			(662936471232937.25, "662936471232937.2"),
			(f64::INFINITY, "null"),
		] {
			let mut out = String::new();
			super::write_number(&mut out, value);
			assert_eq!(out, text, "{value:?}");
		}
	}
}
