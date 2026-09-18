//! The tree's memory layout: every kind's fields by byte offset and type, and how the compiler
//! spells a missing optional. `json::layout_json` tells it to a front end.

use crate::ast::{List, NodeId};
use crate::interner::StrId;
use crate::names::Name;

#[derive(Clone, Copy)]
pub enum Ty {
	Node,
	OptNode,
	List,
	OptList,
	Str,
	OptStr,
	Bool,
	OptBool,
	U32,
	OptU32,
	/// Two words: a range.
	Pair,
	Enum(&'static [Name]),
	OptEnum(&'static [Name]),
	/// A record inside the record, its fields' offsets from its own start.
	Struct(&'static [Field]),
}

#[derive(Clone, Copy)]
pub struct Field {
	pub name: &'static str,
	/// Byte offset from the start of the record.
	pub at: usize,
	pub ty: Ty,
}

#[derive(Clone, Copy)]
pub struct Variant {
	pub name: &'static str,
	/// Offsets from the start of the enum: past its tag word.
	pub fields: &'static [Field],
}

/// What a field's type is on the wire.
pub trait Described {
	const TY: Ty;
}

impl Described for NodeId {
	const TY: Ty = Ty::Node;
}
impl Described for Option<NodeId> {
	const TY: Ty = Ty::OptNode;
}
impl Described for List {
	const TY: Ty = Ty::List;
}
impl Described for Option<List> {
	const TY: Ty = Ty::OptList;
}
impl Described for StrId {
	const TY: Ty = Ty::Str;
}
impl Described for Option<StrId> {
	const TY: Ty = Ty::OptStr;
}
impl Described for bool {
	const TY: Ty = Ty::Bool;
}
impl Described for Option<bool> {
	const TY: Ty = Ty::OptBool;
}
impl Described for u32 {
	const TY: Ty = Ty::U32;
}
impl Described for Option<u32> {
	const TY: Ty = Ty::OptU32;
}
impl Described for [u32; 2] {
	const TY: Ty = Ty::Pair;
}

/// An enum of names: `repr(u8)` in declaration order, each variant spelled as the writer emits it.
macro_rules! names {
	($(#[$m:meta])* $vis:vis enum $E:ident { $( $(#[$vm:meta])* $V:ident = $text:literal ),* $(,)? }) => {
		$(#[$m])*
		#[repr(u8)]
		$vis enum $E {
			$( $(#[$vm])* $V ),*
		}

		impl $E {
			pub const NAMES: &[$crate::names::Name] = &[ $( $crate::names::c!($text) ),* ];

			pub fn name(self) -> $crate::names::Name {
				Self::NAMES[self as usize]
			}

			pub fn as_str(self) -> &'static str {
				self.name().text
			}
		}

		impl $crate::layout::Described for $E {
			const TY: $crate::layout::Ty = $crate::layout::Ty::Enum($E::NAMES);
		}

		impl $crate::layout::Described for Option<$E> {
			const TY: $crate::layout::Ty = $crate::layout::Ty::OptEnum($E::NAMES);
		}
	};
}
pub(crate) use names;

/// A `repr(C)` record whose fields are described.
macro_rules! record {
	($(#[$m:meta])* $vis:vis struct $S:ident { $( $(#[$fm:meta])* $fv:vis $f:ident : $t:ty ),* $(,)? }) => {
		$(#[$m])*
		#[repr(C)]
		$vis struct $S {
			$( $(#[$fm])* $fv $f: $t ),*
		}

		impl $S {
			pub const FIELDS: &[$crate::layout::Field] = &[ $(
				$crate::layout::Field {
					name: stringify!($f),
					at: std::mem::offset_of!($S, $f),
					ty: <$t as $crate::layout::Described>::TY,
				}
			),* ];
		}

		impl $crate::layout::Described for $S {
			const TY: $crate::layout::Ty = $crate::layout::Ty::Struct($S::FIELDS);
		}
	};
}
pub(crate) use record;

/// A `repr(C, u32)` enum of node kinds: a tag word, then the variant's fields as a `repr(C)`
/// record. The module named after `in` holds one such record per variant, for the offsets, and
/// `VARIANTS` in tag order.
macro_rules! kinds {
	(
		$(#[$m:meta])* $vis:vis enum $E:ident in $layout:ident {
			$( $(#[$vm:meta])* $V:ident $( { $( $(#[$fm:meta])* $f:ident : $t:ty ),* $(,)? } )? $( ( $tt:ty ) )? ),* $(,)?
		}
	) => {
		$(#[$m])*
		#[repr(C, u32)]
		$vis enum $E {
			$( $(#[$vm])* $V $( { $( $(#[$fm])* $f: $t ),* } )? $( ( $tt ) )? ),*
		}

		#[allow(dead_code, non_snake_case)]
		pub mod $layout {
			use $crate::layout::Variant;

			// a module per variant: its record, and the fields with the enum's types in scope
			$(
				#[allow(unused_imports)]
				pub mod $V {
					use super::super::*;
					use $crate::layout::{Described, Field};

					#[repr(C)]
					pub struct Record { $( $( pub $f: $t ),* )? $( pub f0: $tt )? }

					pub const FIELDS: &[Field] = &[
						$( $( Field {
							name: stringify!($f),
							at: 4 + std::mem::offset_of!(Record, $f),
							ty: <$t as Described>::TY,
						} ),* )?
						$( Field {
							name: "0",
							at: 4 + std::mem::offset_of!(Record, f0),
							ty: <$tt as Described>::TY,
						} )?
					];
				}
			)*

			pub const VARIANTS: &[Variant] = &[ $(
				Variant {
					name: stringify!($V),
					fields: $V::FIELDS,
				}
			),* ];
		}
	};
}
pub(crate) use kinds;

/// How the compiler spells a missing optional: the byte of a missing enum or bool, and for a
/// list or a string id, which word of the field is the tag and what it holds when missing; the
/// value's words follow in their order around it.
pub struct Missing {
	pub r#enum: u8,
	pub bool: u8,
	pub list: Tagged,
	pub str: Tagged,
	pub int: Tagged,
}

pub struct Tagged {
	pub tag: usize,
	pub missing: u32,
}

fn tagged<const N: usize>(some: [u32; N], missing: [u32; N], values: &[u32]) -> Tagged {
	let tag = (0..N).find(|&i| !values.contains(&some[i])).expect("a tag word");
	for (i, value) in some
		.iter()
		.enumerate()
		.filter(|&(i, _)| i != tag)
		.map(|(_, &w)| w)
		.enumerate()
	{
		assert_eq!(value, values[i], "the value's words follow their order");
	}
	Tagged {
		tag,
		missing: missing[tag],
	}
}

pub fn missing() -> Missing {
	unsafe {
		Missing {
			r#enum: std::mem::transmute::<Option<crate::ast::PropertyKind>, u8>(None),
			bool: std::mem::transmute::<Option<bool>, u8>(None),
			list: tagged(
				std::mem::transmute::<Option<List>, [u32; 3]>(Some(List { start: 7, len: 9 })),
				std::mem::transmute::<Option<List>, [u32; 3]>(None),
				&[7, 9],
			),
			str: tagged(
				std::mem::transmute::<Option<StrId>, [u32; 2]>(Some(StrId::at(7))),
				std::mem::transmute::<Option<StrId>, [u32; 2]>(None),
				&[7],
			),
			int: tagged(
				std::mem::transmute::<Option<u32>, [u32; 2]>(Some(7)),
				std::mem::transmute::<Option<u32>, [u32; 2]>(None),
				&[7],
			),
		}
	}
}
