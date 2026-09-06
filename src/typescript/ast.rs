use crate::ast::{Ast, List, NodeId, NodeKind, Walk};
use crate::interner::StrId;
use crate::scopes::{Bind, Binder, BindingKind, Mode, ScopeKind};

/// What the TypeScript extension hands back with a tree: its own nodes, indexed by the
/// `NodeKind::Extension` payload, and the keys it adds to JavaScript nodes.
#[derive(Debug, Default)]
pub struct Data {
	pub nodes: Vec<TsKind>,
	pub extras: ExtrasTable,
}

impl Data {
	pub fn kind(&self, index: u32) -> TsKind {
		self.nodes[index as usize]
	}

	pub fn extras(&self, id: NodeId) -> Option<&Extras> {
		self.extras.get(id)
	}
}

/// The extras of each node that has any, found through a slot per node id.
#[derive(Debug, Default)]
pub struct ExtrasTable {
	slots: Vec<u32>,
	list: Vec<Extras>,
}

const NONE: u32 = u32::MAX;

impl ExtrasTable {
	pub fn get(&self, id: NodeId) -> Option<&Extras> {
		match self.slots.get(id.0 as usize) {
			Some(&slot) if slot != NONE => Some(&self.list[slot as usize]),
			_ => None,
		}
	}

	pub fn get_or_insert(&mut self, id: NodeId) -> &mut Extras {
		let index = id.0 as usize;
		if index >= self.slots.len() {
			self.slots.resize(index + 1, NONE);
		}
		if self.slots[index] == NONE {
			self.slots[index] = self.list.len() as u32;
			self.list.push(Extras::default());
		}
		&mut self.list[self.slots[index] as usize]
	}

	pub fn is_empty(&self) -> bool {
		self.list.is_empty()
	}
}

/// Whether an import or export carries values or only types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	Value,
	Type,
}

impl Kind {
	pub fn as_str(self) -> &'static str {
		match self {
			Kind::Value => "value",
			Kind::Type => "type",
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accessibility {
	Public,
	Private,
	Protected,
}

impl Accessibility {
	pub fn as_str(self) -> &'static str {
		match self {
			Accessibility::Public => "public",
			Accessibility::Private => "private",
			Accessibility::Protected => "protected",
		}
	}
}

/// A `readonly` or `?` modifier of a mapped type: bare, or with a `+`/`-` prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
	Plus,
	Minus,
	True,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureKind {
	Method,
	Get,
	Set,
}

impl SignatureKind {
	pub fn as_str(self) -> &'static str {
		match self {
			SignatureKind::Method => "method",
			SignatureKind::Get => "get",
			SignatureKind::Set => "set",
		}
	}
}

/// Keys TypeScript adds to a JavaScript node, or modifiers on one of its own. Serialized only
/// when set.
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
	pub accessor: bool,
	pub is_static: bool,
}

impl Extras {
	pub(crate) fn merge(&mut self, other: Extras) {
		macro_rules! take {
			($($field:ident),*) => { $( if other.$field.is_some() { self.$field = other.$field; } )* };
		}
		take!(
			type_annotation,
			return_type,
			type_parameters,
			type_arguments,
			super_type_arguments,
			implements,
			decorators,
			accessibility,
			import_kind,
			export_kind
		);
		self.optional |= other.optional;
		self.definite |= other.definite;
		self.declare |= other.declare;
		self.is_abstract |= other.is_abstract;
		self.readonly |= other.readonly;
		self.is_override |= other.is_override;
		self.accessor |= other.accessor;
		self.is_static |= other.is_static;
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TsKind {
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
		computed: Option<bool>,
		optional: bool,
		readonly: bool,
		kind: Option<SignatureKind>,
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
	ExpressionWithTypeArguments {
		expression: NodeId,
		type_arguments: Option<NodeId>,
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
	/// `expr: type` inside parentheses, before it is known to be a parameter.
	TypeCastExpression {
		expression: NodeId,
		type_annotation: NodeId,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
	Any,
	Boolean,
	BigInt,
	Never,
	Number,
	Object,
	String,
	Symbol,
	Undefined,
	Unknown,
	Void,
	Null,
	Intrinsic,
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

	pub fn estree_type(self) -> &'static str {
		match self {
			Keyword::Any => "TSAnyKeyword",
			Keyword::Boolean => "TSBooleanKeyword",
			Keyword::BigInt => "TSBigIntKeyword",
			Keyword::Never => "TSNeverKeyword",
			Keyword::Number => "TSNumberKeyword",
			Keyword::Object => "TSObjectKeyword",
			Keyword::String => "TSStringKeyword",
			Keyword::Symbol => "TSSymbolKeyword",
			Keyword::Undefined => "TSUndefinedKeyword",
			Keyword::Unknown => "TSUnknownKeyword",
			Keyword::Void => "TSVoidKeyword",
			Keyword::Null => "TSNullKeyword",
			Keyword::Intrinsic => "TSIntrinsicKeyword",
		}
	}
}

impl Data {
	fn index(ast: &Ast<Self>, id: NodeId) -> Option<u32> {
		match ast.node(id).kind {
			NodeKind::Extension(index) => Some(index),
			_ => None,
		}
	}

	fn root(&self, ast: &Ast<Self>, mut id: NodeId) -> Option<NodeId> {
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

impl Bind for Data {
	fn bind(&self, b: &mut Binder<Self>, id: NodeId, mode: Mode) {
		use TsKind::*;
		let Some(index) = Self::index(b.ast(), id) else { return };
		match self.kind(index) {
			AsExpression { expression, .. }
			| SatisfiesExpression { expression, .. }
			| NonNullExpression { expression }
			| TypeAssertion { expression, .. }
			| TypeCastExpression { expression, .. }
			| InstantiationExpression { expression, .. }
			| ParameterProperty { parameter: expression } => b.visit(expression, mode),
			Decorator { expression } | ExportAssignment { expression } => b.visit(expression, Mode::Expression),
			EnumDeclaration { id: name, members, .. } => {
				b.declare(name, BindingKind::Enum);
				b.enter_owned(ScopeKind::Enum, id, b.declared_by(name));
				let members: Vec<_> = b.ast().list(members).iter().flatten().copied().collect();
				for &member in &members {
					if let Some(EnumMember { id: name, .. }) = Self::index(b.ast(), member).map(|i| self.kind(i)) {
						b.declare(name, BindingKind::EnumMember);
					}
				}
				for &member in &members {
					if let Some(EnumMember {
						initializer: Some(initializer),
						..
					}) = Self::index(b.ast(), member).map(|i| self.kind(i))
					{
						b.visit(initializer, Mode::Expression);
					}
				}
				b.exit();
			}
			ModuleDeclaration { id: name, body, global } => {
				if !global {
					b.declare(name, BindingKind::Namespace);
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
				..
			} => {
				b.declare(name, BindingKind::Import);
				if let Some(root) = self.root(b.ast(), module_reference) {
					b.reference(root, false, false);
				}
			}
			// an overload or an ambient signature names the function like its implementation would
			DeclareFunction { id: Some(name), .. } => b.declare(name, BindingKind::Function),
			_ => {}
		}
	}

	fn bind_extras(&self, b: &mut Binder<Self>, id: NodeId) {
		if let Some(decorators) = self.extras(id).and_then(|e| e.decorators) {
			for &decorator in b.ast().list(decorators).iter().flatten() {
				b.visit(decorator, Mode::Expression);
			}
		}
	}

	fn types_only(&self, _ast: &Ast<Self>, id: NodeId) -> bool {
		self.extras(id)
			.is_some_and(|e| e.import_kind == Some(Kind::Type) || e.export_kind == Some(Kind::Type))
	}

	fn wrapped(&self, ast: &Ast<Self>, id: NodeId) -> Option<NodeId> {
		use TsKind::*;
		match self.kind(Self::index(ast, id)?) {
			AsExpression { expression, .. }
			| SatisfiesExpression { expression, .. }
			| NonNullExpression { expression }
			| TypeAssertion { expression, .. }
			| TypeCastExpression { expression, .. }
			| InstantiationExpression { expression, .. } => Some(expression),
			_ => None,
		}
	}
}

impl Walk for Data {
	fn children(&self, ast: &Ast<Self>, id: NodeId, out: &mut Vec<NodeId>) {
		let list = |list: Option<List>, out: &mut Vec<NodeId>| {
			if let Some(list) = list {
				out.extend(ast.list(list).iter().flatten());
			}
		};
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
					TypeParameterInstantiation { params } | TypeParameterDeclaration { params } => {
						list(Some(params), out)
					}
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
						list(Some(parameters), out);
						out.push(type_annotation);
					}
					UnionType { types } | IntersectionType { types } => list(Some(types), out),
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
					TypeLiteral { members } => list(Some(members), out),
					NamedTupleMember {
						label, element_type, ..
					} => out.extend([label, element_type]),
					TupleType { element_types } => list(Some(element_types), out),
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
						list(Some(parameters), out);
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
						list(Some(parameters), out);
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
						list(Some(parameters), out);
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
						list(extends, out);
						out.push(body);
					}
					InterfaceBody { body } | ModuleBlock { body } => list(Some(body), out),
					ExpressionWithTypeArguments {
						expression,
						type_arguments,
					} => {
						out.push(expression);
						out.extend(type_arguments);
					}
					EnumDeclaration { id, members, .. } => {
						out.push(id);
						list(Some(members), out);
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
						list(Some(params), out);
						out.extend(extras.return_type);
					}
					DeclareMethod { params, .. } => {
						list(Some(params), out);
						out.extend(extras.return_type);
					}
					AsExpression {
						expression,
						type_annotation,
					}
					| SatisfiesExpression {
						expression,
						type_annotation,
					}
					| TypeCastExpression {
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
				list(extras.implements, out);
				list(extras.decorators, out);
			}
		}
	}
}
