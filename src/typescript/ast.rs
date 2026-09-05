use crate::ast::{List, NodeId};
use crate::interner::StrId;
use std::collections::HashMap;

/// What the TypeScript extension hands back with a tree: its own nodes, indexed by the
/// `NodeKind::Extension` payload, and the keys it adds to JavaScript nodes.
#[derive(Debug, Default)]
pub struct Data {
	pub nodes: Vec<TsKind>,
	pub extras: HashMap<NodeId, Extras>,
}

impl Data {
	pub fn kind(&self, index: u32) -> TsKind {
		self.nodes[index as usize]
	}

	pub fn extras(&self, id: NodeId) -> Option<&Extras> {
		self.extras.get(&id)
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
	/// An object literal method's type parameters come after its body in the plugin's property
	/// order, which walkers follow.
	pub type_parameters_after_body: bool,
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
		self.type_parameters_after_body |= other.type_parameters_after_body;
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
