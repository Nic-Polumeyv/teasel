//! The host layer: a Svelte component or a CSS stylesheet, the JavaScript inside read by the
//! parser, one tree for both. `core` knows tags, text and where JavaScript begins; `svelte` the
//! rest of Svelte, `css` the stylesheets.

mod core;
mod css;
pub(crate) mod entities;
pub(crate) mod svelte;

pub(crate) use self::core::own;
use self::core::Core;
use crate::ast::{Ast, NodeId};
use crate::parser::{Options, Result};

/// Every type of node a stylesheet holds, each with the fields that hold nodes.
pub(crate) fn stylesheet_children() -> Vec<(&'static str, Vec<&'static str>)> {
	css::children(false)
}

/// The whole source as a CSS stylesheet; see `Core::stylesheet`. Its rows are numbered among
/// Svelte's, by the same names.
pub(crate) fn parse_stylesheet(src: &str, options: Options, reused: Option<Box<Ast>>) -> (Box<Ast>, Result<NodeId>) {
	svelte::SHAPES.with(|shapes| {
		let mut core = Core::new(src, src.len() as u32, shapes.borrow_mut(), options, reused);
		let root = core.stylesheet();
		(core.ast.take().unwrap(), root)
	})
}
