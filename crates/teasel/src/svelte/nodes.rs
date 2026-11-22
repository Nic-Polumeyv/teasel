//! Text and fragments, the nodes every part of the walker writes.

use std::borrow::Cow;

use super::Walker;
use super::entities::decode;
use crate::ast::{List, NodeId, NodeKind};

impl Walker<'_> {
	pub(crate) fn text_chunk(&mut self, start: u32, end: u32, attribute: bool) -> NodeId {
		let raw = &self.src[start as usize..end as usize];
		let kind = match decode(raw, attribute) {
			Cow::Borrowed(_) => NodeKind::Text,
			Cow::Owned(data) => NodeKind::TextDecoded {
				data: self.intern(&data),
			},
		};
		self.ast().add(kind, start, end)
	}

	/// A text's `data`: its source, or what its character references decode to.
	pub(crate) fn text_data(&self, id: NodeId) -> Option<&str> {
		let node = self.tree().node(id);
		match node.kind {
			NodeKind::Text => Some(&self.src[node.start as usize..node.end as usize]),
			NodeKind::TextDecoded { data } => Some(self.tree().str(data)),
			_ => None,
		}
	}

	pub(crate) fn flush_text(&mut self, from: u32, to: u32, chunks: &mut Vec<NodeId>) {
		if to > from {
			let text = self.text_chunk(from, to, true);
			chunks.push(text);
		}
	}

	/// The nodes as a fragment: positioned for the walks that order nodes, though none are written,
	/// at the cursor when there are none. The buffer is given back.
	pub(super) fn fragment(&mut self, nodes: Vec<NodeId>) -> NodeId {
		let list = self.list(&nodes);
		let (start, end) = match (nodes.first(), nodes.last()) {
			(Some(&first), Some(&last)) => (self.tree().node(first).start, self.tree().node(last).end),
			_ => (self.at, self.at),
		};
		self.nodes.give(nodes);
		self.ast().add(NodeKind::Fragment { nodes: list }, start, end)
	}

	pub(crate) fn list(&mut self, nodes: &[NodeId]) -> List {
		self.ast().add_list_from(nodes.iter().map(|&id| Some(id)))
	}
}
