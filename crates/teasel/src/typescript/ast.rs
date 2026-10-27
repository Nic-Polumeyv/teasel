use crate::ast::{Ast, List, NodeId, NodeKind};
use crate::estree::Writer;
use crate::handed::{Handed, Views};
use crate::interner::StrId;
use crate::layout::Ty;
use crate::recipe::{ADDS, EXTRAS_ERASED, Op, Slot, TS, TS_ADDS};
use crate::scopes::{Binder, BindingKind, Mode, ScopeKind};

/// What TypeScript hands back with a tree: its own nodes, indexed by the `NodeKind::Extension`
/// payload, and the keys it adds to JavaScript nodes.
#[derive(Debug, Default)]
pub struct Data {
	pub nodes: Handed<TsKind>,
	pub extras: crate::ast::Slots<Extras>,
}

impl Data {
	pub fn kind(&self, index: u32) -> TsKind {
		self.nodes[index as usize]
	}

	pub fn extras(&self, id: NodeId) -> Option<&Extras> {
		self.extras.get(id)
	}
}

impl Data {
	/// Forgets everything, keeping the room.
	pub(crate) fn clear(&mut self) {
		self.nodes.clear();
		self.extras.clear();
	}

	/// Where the tables stood, to cut back to.
	pub(crate) fn mark(&self) -> (usize, usize) {
		(self.nodes.len(), self.extras.len())
	}

	pub(crate) fn truncate(&mut self, (nodes, extras): (usize, usize)) {
		self.nodes.truncate(nodes);
		self.extras.truncate(extras);
	}

	/// The tables' buffers, after the tree's.
	pub(crate) fn views(&mut self, out: &mut Views<'_>) {
		out.push("ts", &mut self.nodes);
		self.extras.views("extras_slots", "extras", out);
	}

	/// The nodes TypeScript adds keys to when erasing.
	pub(crate) fn rare(&self, set: &mut crate::ast::NodeSet) {
		for &owner in self.extras.owners() {
			if self.extras(owner).is_some_and(|extras| extras.decorators.is_some()) {
				set.insert(owner);
			}
		}
	}

	/// Whether erasing leaves nothing of a node in a list: type-only declarations and imports.
	pub(crate) fn erased(&self, ast: &Ast, id: NodeId) -> bool {
		use TsKind::*;
		let extras = self.extras_of(id);
		if extras.declare {
			return true;
		}
		match ast.node(id).kind {
			NodeKind::MethodDefinition { .. } | NodeKind::PropertyDefinition { .. } if extras.is_abstract => true,
			// an overload signature: a method without a body
			NodeKind::MethodDefinition { value, .. } => {
				matches!(self.ts_of(ast, value), Some(DeclareMethod { .. }))
			}
			NodeKind::Extension(index) => match self.kind(index) {
				InterfaceDeclaration { .. }
				| TypeAliasDeclaration { .. }
				| DeclareFunction { .. }
				| IndexSignature { .. }
				| NamespaceExportDeclaration { .. } => true,
				ImportEqualsDeclaration { import_kind, .. } => import_kind == Kind::Type,
				ModuleDeclaration { body: None, .. } => true,
				ModuleDeclaration { body: Some(block), .. } => match self.ts_of(ast, block) {
					Some(ModuleBlock { body }) => self.all_erased(ast, body),
					_ => false,
				},
				_ => false,
			},
			NodeKind::ImportDeclaration { specifiers, .. } => {
				extras.import_kind == Some(Kind::Type) || (specifiers.len > 0 && self.all_erased(ast, specifiers))
			}
			NodeKind::ImportSpecifier { .. } => extras.import_kind == Some(Kind::Type),
			NodeKind::ExportSpecifier { .. } | NodeKind::ExportAllDeclaration { .. } => {
				extras.export_kind == Some(Kind::Type)
			}
			// `export { type A }` keeps an `export {}`, as tsc keeps the file a module
			NodeKind::ExportDeclaration { declaration } => {
				extras.export_kind == Some(Kind::Type) || self.erased(ast, declaration)
			}
			NodeKind::ExportNamedDeclaration { .. } => extras.export_kind == Some(Kind::Type),
			NodeKind::ExportDefaultDeclaration { declaration } => {
				extras.export_kind == Some(Kind::Type) || self.erased(ast, declaration)
			}
			_ => false,
		}
	}
}

crate::layout::names! {
	/// Whether an import or export carries values or only types.
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Kind {
		Value = "value",
		Type = "type",
	}
}

crate::layout::names! {
	/// An interface's `extends` or a class's `implements`.
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Clause {
		Extends = "TSInterfaceHeritage",
		Implements = "TSClassImplements",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Accessibility {
		Public = "public",
		Private = "private",
		Protected = "protected",
	}
}

crate::layout::names! {
	/// A `readonly` or `?` modifier of a mapped type: bare, or with a `+`/`-` prefix.
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Modifier {
		Plus = "+",
		Minus = "-",
		True = "true",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum SignatureKind {
		Method = "method",
		Get = "get",
		Set = "set",
	}
}

crate::layout::record! {
	/// Keys TypeScript adds to a JavaScript node, or modifiers on one of its own.
	#[derive(Clone, Copy, Debug, Default, PartialEq)]
	pub struct Extras {
		pub type_annotation: Option<NodeId>,
		pub return_type: Option<NodeId>,
		pub type_parameters: Option<NodeId>,
		pub type_arguments: Option<NodeId>,
		pub super_type_arguments: Option<NodeId>,
		pub implements: Option<List>,
		pub decorators: Option<List>,
		pub accessibility: Option<Accessibility>,
		pub import_kind: Option<Kind>,
		pub export_kind: Option<Kind>,
		pub optional: bool,
		pub definite: bool,
		pub declare: bool,
		pub is_abstract: bool,
		pub readonly: bool,
		pub is_override: bool,
		pub is_static: bool,
	}
}

impl Extras {
	fn set(&self) -> impl Iterator<Item = &'static str> {
		let base = self as *const Extras as *const u8;
		Extras::FIELDS
			.iter()
			.filter(move |field| unsafe {
				let at = base.add(field.at);
				match field.ty {
					Ty::Bool => at.cast::<bool>().read(),
					Ty::OptNode => at.cast::<Option<NodeId>>().read().is_some(),
					Ty::OptList => at.cast::<Option<List>>().read().is_some(),
					Ty::OptEnum(names) => usize::from(at.read()) < names.len(),
					_ => unreachable!(),
				}
			})
			.map(|field| field.name)
	}
}

crate::layout::kinds! {
	#[derive(Clone, Copy, Debug, PartialEq)]
	pub enum TsKind in ts_layout {
		// Types
		TypeAnnotation {
			type_annotation: NodeId,
		},
		/// `any`, `string`, ... and `void`, `null`, `intrinsic`; the name is the ESTree type.
		Keyword(Keyword),
		ThisType,
		TypePredicate {
			parameter_name: NodeId,
			type_annotation: Option<NodeId>,
			asserts: bool,
		},
		TypeReference {
			type_name: NodeId,
			type_arguments: Option<NodeId>,
		},
		QualifiedName {
			left: NodeId,
			right: NodeId,
		},
		TypeParameterInstantiation {
			params: List,
		},
		TypeParameterDeclaration {
			params: List,
		},
		TypeParameter {
			name: StrId,
			constraint: Option<NodeId>,
			default: Option<NodeId>,
			is_in: bool,
			is_out: bool,
			is_const: bool,
		},
		FunctionType {
			type_parameters: Option<NodeId>,
			parameters: List,
			type_annotation: NodeId,
		},
		ConstructorType {
			type_parameters: Option<NodeId>,
			parameters: List,
			type_annotation: NodeId,
			is_abstract: bool,
		},
		UnionType {
			types: List,
		},
		IntersectionType {
			types: List,
		},
		TypeOperator {
			operator: StrId,
			type_annotation: NodeId,
		},
		InferType {
			type_parameter: NodeId,
		},
		LiteralType {
			literal: NodeId,
		},
		ImportType {
			argument: NodeId,
			qualifier: Option<NodeId>,
			type_arguments: Option<NodeId>,
		},
		TypeQuery {
			expr_name: NodeId,
			type_arguments: Option<NodeId>,
		},
		MappedType {
			readonly: Option<Modifier>,
			type_parameter: NodeId,
			name_type: Option<NodeId>,
			optional: Option<Modifier>,
			type_annotation: Option<NodeId>,
		},
		TypeLiteral {
			members: List,
		},
		NamedTupleMember {
			label: NodeId,
			optional: bool,
			element_type: NodeId,
		},
		OptionalType {
			type_annotation: NodeId,
		},
		RestType {
			type_annotation: NodeId,
		},
		TupleType {
			element_types: List,
		},
		ParenthesizedType {
			type_annotation: NodeId,
		},
		ArrayType {
			element_type: NodeId,
		},
		IndexedAccessType {
			object_type: NodeId,
			index_type: NodeId,
		},
		ConditionalType {
			check_type: NodeId,
			extends_type: NodeId,
			true_type: NodeId,
			false_type: NodeId,
		},

		// Type members
		IndexSignature {
			parameters: List,
			type_annotation: Option<NodeId>,
		},
		CallSignatureDeclaration {
			type_parameters: Option<NodeId>,
			parameters: List,
			type_annotation: Option<NodeId>,
		},
		ConstructSignatureDeclaration {
			type_parameters: Option<NodeId>,
			parameters: List,
			type_annotation: Option<NodeId>,
		},
		MethodSignature {
			key: NodeId,
			computed: bool,
			optional: bool,
			kind: SignatureKind,
			type_parameters: Option<NodeId>,
			parameters: List,
			type_annotation: Option<NodeId>,
		},
		PropertySignature {
			key: NodeId,
			computed: bool,
			optional: bool,
			readonly: bool,
			type_annotation: Option<NodeId>,
		},

		// Declarations
		InterfaceDeclaration {
			id: NodeId,
			type_parameters: Option<NodeId>,
			extends: Option<List>,
			body: NodeId,
		},
		InterfaceBody {
			body: List,
		},
		Heritage {
			expression: NodeId,
			type_arguments: Option<NodeId>,
			clause: Clause,
		},
		EnumDeclaration {
			id: NodeId,
			members: List,
			is_const: bool,
		},
		EnumMember {
			id: NodeId,
			initializer: Option<NodeId>,
		},
		ModuleDeclaration {
			id: NodeId,
			body: Option<NodeId>,
			global: bool,
		},
		ModuleBlock {
			body: List,
		},
		TypeAliasDeclaration {
			id: NodeId,
			type_parameters: Option<NodeId>,
			type_annotation: NodeId,
		},
		ImportEqualsDeclaration {
			id: NodeId,
			module_reference: NodeId,
			is_export: bool,
			import_kind: Kind,
		},
		ExternalModuleReference {
			expression: NodeId,
		},
		ExportAssignment {
			expression: NodeId,
		},
		NamespaceExportDeclaration {
			id: NodeId,
		},
		/// A function declaration without a body: an overload or an ambient declaration.
		DeclareFunction {
			id: Option<NodeId>,
			params: List,
			is_async: bool,
			generator: bool,
		},
		/// A method without a body, the `value` of its `MethodDefinition`.
		DeclareMethod {
			params: List,
			is_async: bool,
			generator: bool,
		},

		// Expressions
		AsExpression {
			expression: NodeId,
			type_annotation: NodeId,
		},
		SatisfiesExpression {
			expression: NodeId,
			type_annotation: NodeId,
		},
		NonNullExpression {
			expression: NodeId,
		},
		TypeAssertion {
			type_annotation: NodeId,
			expression: NodeId,
		},
		InstantiationExpression {
			expression: NodeId,
			type_arguments: NodeId,
		},
		ParameterProperty {
			parameter: NodeId,
		},
		Decorator {
			expression: NodeId,
		},
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Keyword {
		Any = "TSAnyKeyword",
		Boolean = "TSBooleanKeyword",
		BigInt = "TSBigIntKeyword",
		Never = "TSNeverKeyword",
		Number = "TSNumberKeyword",
		Object = "TSObjectKeyword",
		String = "TSStringKeyword",
		Symbol = "TSSymbolKeyword",
		Undefined = "TSUndefinedKeyword",
		Unknown = "TSUnknownKeyword",
		Void = "TSVoidKeyword",
		Null = "TSNullKeyword",
		Intrinsic = "TSIntrinsicKeyword",
	}
}

impl Keyword {
	pub(crate) fn from_name(name: &str) -> Option<Keyword> {
		Some(match name {
			"any" => Keyword::Any,
			"boolean" => Keyword::Boolean,
			"bigint" => Keyword::BigInt,
			"never" => Keyword::Never,
			"number" => Keyword::Number,
			"object" => Keyword::Object,
			"string" => Keyword::String,
			"symbol" => Keyword::Symbol,
			"undefined" => Keyword::Undefined,
			"unknown" => Keyword::Unknown,
			_ => return None,
		})
	}
}

impl Data {
	/// The TypeScript node `id` is, if it is one.
	pub(crate) fn ts_of(&self, ast: &Ast, id: NodeId) -> Option<TsKind> {
		match ast.node(id).kind {
			NodeKind::Extension(index) => Some(self.kind(index)),
			_ => None,
		}
	}

	fn root(&self, ast: &Ast, mut id: NodeId) -> Option<NodeId> {
		loop {
			match ast.node(id).kind {
				NodeKind::Identifier { .. } => return Some(id),
				NodeKind::Extension(index) => match self.kind(index) {
					TsKind::QualifiedName { left, .. } => id = left,
					_ => return None,
				},
				_ => return None,
			}
		}
	}
}

impl Data {
	/// Binds what a TypeScript node declares and refers to.
	pub(crate) fn bind(&self, b: &mut Binder, id: NodeId, mode: Mode) {
		use TsKind::*;
		let Some(kind) = self.ts_of(b.ast(), id) else { return };
		match kind {
			AsExpression { expression, .. }
			| SatisfiesExpression { expression, .. }
			| NonNullExpression { expression }
			| TypeAssertion { expression, .. }
			| InstantiationExpression { expression, .. }
			| ParameterProperty { parameter: expression } => b.visit(expression, mode),
			Decorator { expression } | ExportAssignment { expression } => b.visit(expression, Mode::Expression),
			EnumDeclaration { id: name, members, .. } => {
				b.declare_by(name, BindingKind::Enum, id);
				b.enter_owned(ScopeKind::Enum, id, b.declared_by(name));
				let members: Vec<_> = b.ast().list(members).iter().flatten().copied().collect();
				for &member in &members {
					if let Some(EnumMember { id: name, .. }) = self.ts_of(b.ast(), member) {
						b.declare_by(name, BindingKind::EnumMember, member);
					}
				}
				for &member in &members {
					if let Some(EnumMember {
						initializer: Some(initializer),
						..
					}) = self.ts_of(b.ast(), member)
					{
						b.visit(initializer, Mode::Expression);
					}
				}
				b.exit();
			}
			ModuleDeclaration { id: name, body, global } => {
				if !global {
					b.declare_by(name, BindingKind::Namespace, id);
				}
				if let Some(body) = body {
					b.enter_owned(ScopeKind::Namespace, id, b.declared_by(name));
					b.visit(body, Mode::Expression);
					b.exit();
				}
			}
			ModuleBlock { body } => b.statements(body),
			ImportEqualsDeclaration {
				id: name,
				module_reference,
				import_kind,
				..
			} if import_kind != Kind::Type => {
				b.declare_by(name, BindingKind::Import, id);
				if let Some(root) = self.root(b.ast(), module_reference) {
					b.reference(root, false, false);
				}
			}
			// an overload or an ambient signature names the function like its implementation would
			DeclareFunction { id: Some(name), .. } => b.declare_by(name, BindingKind::Function, id),
			_ => {}
		}
	}

	/// The value-space parts TypeScript attaches to a plain node: decorators.
	pub(crate) fn bind_extras(&self, b: &mut Binder, id: NodeId) {
		if let Some(decorators) = self.extras(id).and_then(|e| e.decorators) {
			for &decorator in b.ast().list(decorators).iter().flatten() {
				b.visit(decorator, Mode::Expression);
			}
		}
	}

	/// Whether an import or export only names types, so it binds and refers to no value.
	pub(crate) fn types_only(&self, id: NodeId) -> bool {
		self.extras(id)
			.is_some_and(|e| e.import_kind == Some(Kind::Type) || e.export_kind == Some(Kind::Type))
	}

	/// The expression a TypeScript node wraps without changing its value: `x as T`, `x!`.
	pub(crate) fn wrapped(&self, ast: &Ast, id: NodeId) -> Option<NodeId> {
		use TsKind::*;
		match self.ts_of(ast, id)? {
			AsExpression { expression, .. }
			| SatisfiesExpression { expression, .. }
			| NonNullExpression { expression }
			| TypeAssertion { expression, .. }
			| InstantiationExpression { expression, .. } => Some(expression),
			_ => None,
		}
	}
}

impl Data {
	/// Pushes the children of `id` in source order, with the nodes TypeScript adds to a plain one.
	pub(crate) fn children(&self, ast: &Ast, id: NodeId, out: &mut Vec<NodeId>) {
		let list = |list: List, out: &mut Vec<NodeId>| out.extend(ast.list(list).iter().flatten());
		let from = out.len();
		let extras = self.extras(id).copied().unwrap_or_default();
		match ast.node(id).kind {
			NodeKind::Extension(index) => {
				use TsKind::*;
				match self.kind(index) {
					TypeAnnotation { type_annotation }
					| TypeOperator { type_annotation, .. }
					| OptionalType { type_annotation }
					| RestType { type_annotation }
					| ParenthesizedType { type_annotation } => out.push(type_annotation),
					Keyword(_) | ThisType => {}
					TypePredicate {
						parameter_name,
						type_annotation,
						..
					} => {
						out.push(parameter_name);
						out.extend(type_annotation);
					}
					TypeReference {
						type_name,
						type_arguments,
					} => {
						out.push(type_name);
						out.extend(type_arguments);
					}
					QualifiedName { left, right } => out.extend([left, right]),
					TypeParameterInstantiation { params } | TypeParameterDeclaration { params } => list(params, out),
					TypeParameter {
						constraint, default, ..
					} => {
						out.extend(constraint);
						out.extend(default);
					}
					FunctionType {
						type_parameters,
						parameters,
						type_annotation,
					}
					| ConstructorType {
						type_parameters,
						parameters,
						type_annotation,
						..
					} => {
						out.extend(type_parameters);
						list(parameters, out);
						out.push(type_annotation);
					}
					UnionType { types } | IntersectionType { types } => list(types, out),
					InferType { type_parameter } => out.push(type_parameter),
					LiteralType { literal } => out.push(literal),
					ImportType {
						argument,
						qualifier,
						type_arguments,
					} => {
						out.push(argument);
						out.extend(qualifier);
						out.extend(type_arguments);
					}
					TypeQuery {
						expr_name,
						type_arguments,
					} => {
						out.push(expr_name);
						out.extend(type_arguments);
					}
					MappedType {
						type_parameter,
						name_type,
						type_annotation,
						..
					} => {
						out.push(type_parameter);
						out.extend(name_type);
						out.extend(type_annotation);
					}
					TypeLiteral { members } => list(members, out),
					NamedTupleMember {
						label, element_type, ..
					} => out.extend([label, element_type]),
					TupleType { element_types } => list(element_types, out),
					ArrayType { element_type } => out.push(element_type),
					IndexedAccessType {
						object_type,
						index_type,
					} => out.extend([object_type, index_type]),
					ConditionalType {
						check_type,
						extends_type,
						true_type,
						false_type,
					} => out.extend([check_type, extends_type, true_type, false_type]),
					IndexSignature {
						parameters,
						type_annotation,
					} => {
						list(parameters, out);
						out.extend(type_annotation);
					}
					CallSignatureDeclaration {
						type_parameters,
						parameters,
						type_annotation,
					}
					| ConstructSignatureDeclaration {
						type_parameters,
						parameters,
						type_annotation,
					} => {
						out.extend(type_parameters);
						list(parameters, out);
						out.extend(type_annotation);
					}
					MethodSignature {
						key,
						type_parameters,
						parameters,
						type_annotation,
						..
					} => {
						out.push(key);
						out.extend(type_parameters);
						list(parameters, out);
						out.extend(type_annotation);
					}
					PropertySignature {
						key, type_annotation, ..
					} => {
						out.push(key);
						out.extend(type_annotation);
					}
					InterfaceDeclaration {
						id,
						type_parameters,
						extends,
						body,
					} => {
						out.push(id);
						out.extend(type_parameters);
						if let Some(extends) = extends {
							list(extends, out);
						}
						out.push(body);
					}
					InterfaceBody { body } | ModuleBlock { body } => list(body, out),
					Heritage {
						expression,
						type_arguments,
						..
					} => {
						out.push(expression);
						out.extend(type_arguments);
					}
					EnumDeclaration { id, members, .. } => {
						out.push(id);
						list(members, out);
					}
					EnumMember { id, initializer } => {
						out.push(id);
						out.extend(initializer);
					}
					ModuleDeclaration { id, body, .. } => {
						out.push(id);
						out.extend(body);
					}
					TypeAliasDeclaration {
						id,
						type_parameters,
						type_annotation,
					} => {
						out.push(id);
						out.extend(type_parameters);
						out.push(type_annotation);
					}
					ImportEqualsDeclaration {
						id, module_reference, ..
					} => out.extend([id, module_reference]),
					ExternalModuleReference { expression }
					| ExportAssignment { expression }
					| NonNullExpression { expression }
					| Decorator { expression } => out.push(expression),
					NamespaceExportDeclaration { id } => out.push(id),
					DeclareFunction { id, params, .. } => {
						out.extend(id);
						out.extend(extras.type_parameters);
						list(params, out);
						out.extend(extras.return_type);
					}
					DeclareMethod { params, .. } => {
						list(params, out);
						out.extend(extras.return_type);
					}
					AsExpression {
						expression,
						type_annotation,
					}
					| SatisfiesExpression {
						expression,
						type_annotation,
					} => out.extend([expression, type_annotation]),
					TypeAssertion {
						type_annotation,
						expression,
					} => out.extend([type_annotation, expression]),
					InstantiationExpression {
						expression,
						type_arguments,
					} => out.extend([expression, type_arguments]),
					ParameterProperty { parameter } => out.push(parameter),
				}
			}
			_ => {
				ast.plain_children(id, out);
				out.extend(extras.type_annotation);
				out.extend(extras.return_type);
				out.extend(extras.type_parameters);
				out.extend(extras.type_arguments);
				out.extend(extras.super_type_arguments);
				for list_of in [extras.implements, extras.decorators].into_iter().flatten() {
					list(list_of, out);
				}
			}
		}
		out[from..].sort_unstable_by_key(|&child| ast.node(child).start);
	}
}

// ESTree output for the TypeScript nodes and the keys TypeScript adds to every node
struct Recipes {
	kinds: &'static [&'static [Op<Slot>]],
	/// By the kind's tag: the operations and the fields they read.
	adds: Vec<(&'static [Op<Slot>], Vec<&'static str>)>,
	ts_adds: Vec<(&'static [Op<Slot>], Vec<&'static str>)>,
	erased: &'static [Op<Slot>],
}

fn recipes() -> &'static Recipes {
	static RESOLVED: std::sync::OnceLock<Recipes> = std::sync::OnceLock::new();
	let adds = |table: &[(&str, &'static [Op])], variants: &[crate::layout::Variant]| {
		variants
			.iter()
			.map(|variant| match table.iter().find(|(name, _)| *name == variant.name) {
				Some((name, ops)) => (
					crate::recipe::resolve_ops(ops, Extras::FIELDS, name),
					ops.iter().filter_map(|op| op.told().2).collect(),
				),
				None => (&[][..], Vec::new()),
			})
			.collect()
	};
	RESOLVED.get_or_init(|| Recipes {
		kinds: crate::recipe::resolve(TS, super::ast::ts_layout::VARIANTS),
		adds: adds(ADDS, crate::ast::node_layout::VARIANTS),
		ts_adds: adds(TS_ADDS, super::ast::ts_layout::VARIANTS),
		erased: crate::recipe::resolve_ops(EXTRAS_ERASED, Extras::FIELDS, "Extras"),
	})
}

impl Data {
	/// Whether every statement of a list erases to nothing.
	fn all_erased(&self, ast: &Ast, list: List) -> bool {
		ast.list(list)
			.iter()
			.all(|item| item.is_none_or(|id| self.erased(ast, id)))
	}

	fn extras_of(&self, id: NodeId) -> Extras {
		self.extras(id).copied().unwrap_or_default()
	}
}

impl Data {
	/// Writes one of TypeScript's own nodes and closes it, or what erasing puts in its place.
	pub(crate) fn write_node(&self, w: &mut Writer, id: NodeId, index: u32) {
		let base = &self.nodes[index as usize] as *const TsKind as *const u8;
		if w.run(id, recipes().kinds[crate::estree::tag(base)], base) {
			w.end();
		}
	}

	/// Writes the keys TypeScript adds to a node.
	pub(crate) fn write_extras(&self, w: &mut Writer, id: NodeId) {
		let recipes = recipes();
		let extras = self.extras_of(id);
		let base = &extras as *const Extras as *const u8;
		if w.options.has(crate::Options::ERASE) {
			w.run(id, recipes.erased, base);
			return;
		}
		let (ops, read) = match w.kind(id) {
			NodeKind::Extension(index) => {
				&recipes.ts_adds[crate::estree::tag(&self.nodes[index as usize] as *const TsKind as *const u8)]
			}
			kind => &recipes.adds[crate::estree::tag(&kind as *const NodeKind as *const u8)],
		};
		w.run(id, ops, base);
		// an unlisted extra is a parser bug, which a test build catches and a shipped build answers around
		if cfg!(debug_assertions)
			&& let Some(field) = extras.set().find(|&field| !read.contains(&field))
		{
			panic!("{:?} has {field}, which its kind does not list", w.kind(id));
		}
	}
}
