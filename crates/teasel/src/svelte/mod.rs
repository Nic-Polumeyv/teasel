//! Svelte: a component or a CSS stylesheet, the JavaScript inside read by the parser, one tree
//! for both. `walker` reads the component, `css` the stylesheets.

mod css;
pub(crate) mod entities;
pub(crate) mod walker;

pub(crate) use self::walker::n;
use self::walker::Walker;
use crate::ast::{Ast, NodeId};
use crate::parser::{Options, Result};

/// Every type of node a stylesheet holds, each with the fields that hold nodes.
pub(crate) fn stylesheet_children() -> Vec<(&'static str, Vec<&'static str>)> {
	css::children(false)
}

/// The whole source as a CSS stylesheet; see `Walker::stylesheet`. Its rows are numbered among
/// Svelte's, by the same names.
pub(crate) fn parse_stylesheet(src: &str, options: Options, reused: Option<Box<Ast>>) -> (Box<Ast>, Result<NodeId>) {
	walker::SHAPES.with(|shapes| {
		let mut walker = Walker::new(src, src.len() as u32, shapes.borrow_mut(), options, reused);
		let root = walker.stylesheet();
		(walker.ast.take().unwrap(), root)
	})
}
