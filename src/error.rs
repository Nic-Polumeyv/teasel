use std::fmt;

macro_rules! codes {
	($($name:ident => $message:literal,)*) => {
		/// What went wrong, as a stable name for hosts to branch on; the message is for people.
		#[derive(Clone, Copy, Debug, PartialEq, Eq)]
		pub enum Code {
			$($name,)*
		}

		impl Code {
			/// The message with its `{}` placeholders, when it has any.
			pub fn message(self) -> &'static str {
				match self {
					$(Code::$name => $message,)*
				}
			}

			/// The variant in snake case: `UnexpectedEof` is `unexpected_eof`.
			pub fn name(self) -> String {
				let variant = match self {
					$(Code::$name => stringify!($name),)*
				};
				let mut out = String::with_capacity(variant.len() + 4);
				for (i, c) in variant.char_indices() {
					if c.is_ascii_uppercase() {
						if i > 0 {
							out.push('_');
						}
						out.push(c.to_ascii_lowercase());
					} else {
						out.push(c);
					}
				}
				out
			}
		}
	};
}

codes! {
	UnexpectedToken => "Unexpected token",
	UnexpectedEof => "Unexpected end of input",
	UnexpectedCharacter => "Unexpected character '{}'",
	UnexpectedKeyword => "Unexpected keyword '{}'",
	ReservedWord => "The keyword '{}' is reserved",
	EscapeInKeyword => "Escape sequence in keyword {}",
	NestingDepth => "Maximum nesting depth exceeded",
	UnterminatedComment => "Unterminated comment",
	UnterminatedString => "Unterminated string constant",
	UnterminatedTemplate => "Unterminated template",
	UnterminatedRegexp => "Unterminated regular expression",
	InvalidRegexp => "Invalid regular expression: /{}/: {}",
	InvalidRegexpFlag => "Invalid regular expression flag",
	DuplicateRegexpFlag => "Duplicate regular expression flag",
	InvalidNumber => "Invalid number",
	ExpectedNumberInRadix => "Expected number in radix {}",
	IdentifierAfterNumber => "Identifier directly after number",
	NumericSeparatorFirst => "Numeric separator is not allowed at the first of digits",
	NumericSeparatorLast => "Numeric separator is not allowed at the last of digits",
	NumericSeparatorDouble => "Numeric separator must be exactly one underscore",
	NumericSeparatorLegacyOctal => "Numeric separator is not allowed in legacy octal numeric literals",
	BadCharacterEscape => "Bad character escape sequence",
	BadTemplateEscape => "Bad escape sequence in untagged template literal",
	InvalidUnicodeEscape => "Invalid Unicode escape",
	ExpectedUnicodeEscape => "Expecting Unicode escape sequence \\uXXXX",
	CodePointOutOfBounds => "Code point out of bounds",
	StrictWith => "'with' in strict mode",
	StrictDelete => "Deleting local variable in strict mode",
	StrictDirectiveNonSimpleParams => "Illegal 'use strict' directive in function with non-simple parameter list",
	StrictBinding => "{}{} in strict mode",
	StrictOctal => "Octal literal in strict mode",
	StrictEscape => "Invalid escape sequence",
	LetAsBinding => "let is disallowed as a lexically bound name",
	Redeclaration => "Identifier '{}' has already been declared",
	DuplicateLabel => "Label '{}' is already declared",
	DuplicateParameter => "Argument name clash",
	DuplicateProto => "Redefinition of __proto__ property",
	DuplicateDefault => "Multiple default clauses",
	DuplicateExport => "Duplicate export '{}'",
	UndefinedExport => "Export '{}' is not defined",
	StringExportWithoutFrom => "A string literal cannot be used as an exported binding without `from`.",
	DuplicateImportAttribute => "Duplicate attribute key '{}'",
	ImportExportInScript => "'import' and 'export' may appear only with 'sourceType: module'",
	ImportExportNotTopLevel => "'import' and 'export' may only appear at the top level",
	ImportMetaOutsideModule => "Cannot use 'import.meta' outside a module",
	ImportMetaEscaped => "'import.meta' must not contain escaped characters",
	InvalidImportMeta => "The only valid meta property for import is 'import.meta'",
	InvalidNewTarget => "The only valid meta property for new is 'new.target'",
	NewTargetEscaped => "'new.target' must not contain escaped characters",
	NewTargetOutsideFunction => "'new.target' can only be used in functions and class static block",
	Unsyntactic => "Unsyntactic {}",
	ReturnOutsideFunction => "'return' outside of function",
	NewlineAfterThrow => "Illegal newline after throw",
	MissingCatchOrFinally => "Missing catch or finally clause",
	ForInOfInitializer => "{} loop variable declaration may not have an initializer",
	ForOfLet => "The left-hand side of a for-of loop may not start with 'let'.",
	AwaitAsIdentifier => "Cannot use 'await' as identifier inside an async function",
	AwaitOutsideAsync => "Cannot use keyword 'await' outside an async function",
	AwaitInDefaultValue => "Await expression cannot be a default value",
	YieldAsIdentifier => "Cannot use 'yield' as identifier inside a generator",
	YieldInDefaultValue => "Yield expression cannot be a default value",
	InvalidInStaticBlock => "Cannot use {} in class static initialization block",
	ArgumentsInFieldInitializer => "Cannot use 'arguments' in class field initializer",
	InvalidAssignmentTarget => "Assigning to rvalue",
	InvalidBindingTarget => "Binding rvalue",
	BindingMemberExpression => "Binding member expression",
	ParenthesizedPattern => "Parenthesized pattern",
	BindingParenthesized => "Binding parenthesized expression",
	OptionalChainAssignment => "Optional chaining cannot appear in left-hand side",
	OptionalChainInNew => "Optional chaining cannot appear in the callee of new expressions",
	OptionalChainInTaggedTemplate => "Optional chaining cannot appear in the tag of tagged template expressions",
	MixedCoalesce => "Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses",
	PrivateNameOutsideIn => "Private identifier can only be left side of binary expression",
	UndeclaredPrivateName => "Private field '#{}' must be declared in an enclosing class",
	DeletePrivate => "Private fields can not be deleted",
	InvalidSuper => "Invalid use of 'super'",
	SuperOutsideMethod => "'super' keyword outside a method",
	SuperCallOutsideConstructor => "super() call outside constructor of a subclass",
	CommaAfterRest => "Comma is not permitted after the rest element",
	RestWithDefault => "Rest elements cannot have a default value",
	AccessorInPattern => "Object pattern can't contain getter or setter",
	ShorthandAssignment => "Shorthand property assignments are valid only in destructuring patterns",
	PatternWithoutInitializer => "Complex binding patterns require an initialization value",
	InvalidDefaultOperator => "Only '=' operator can be used for specifying default value.",
	GetterParams => "getter should have no params",
	SetterParams => "setter should have exactly one param",
	SetterRestParam => "Setter cannot use rest params",
	GeneratorConstructor => "Constructor can't be a generator",
	AsyncConstructor => "Constructor can't be an async method",
	AccessorConstructor => "Constructor can't have get/set modifier",
	DuplicateConstructor => "Duplicate constructor in the same class",
	ConstructorField => "Classes can't have a field named 'constructor'",
	PrivateConstructor => "Classes can't have an element named '#constructor'",
	StaticPrototype => "Classes can't have a static field named 'prototype'",
	AbstractOutsideAbstractClass => "Abstract methods can only appear within an abstract class.",
	AbstractWithImplementation => "Method '{}' cannot have an implementation because it is marked abstract.",
	AbstractWithInitializer => "Property '{}' cannot have an initializer because it is marked abstract.",
	DuplicateAccessibility => "Accessibility modifier already seen.",
	DuplicateModifier => "Duplicate modifier: '{}'.",
	ConflictingModifiers => "'{}' modifier cannot be used with '{}' modifier.",
	ModifierOrder => "'{}' modifier must precede '{}' modifier.",
	DeclareOnMethod => "Class methods cannot have the 'declare' modifier.",
	ReadonlyOnMethod => "Class methods cannot have the 'readonly' modifier.",
	ReadonlyPlacement => "'readonly' modifier can only appear on a property declaration or index signature.",
	ReadonlyTypeOperand => "'readonly' type modifier is only permitted on array and tuple literal types.",
	OverrideWithoutExtends => "This member cannot have an 'override' modifier because its containing class does not extend another class.",
	IndexSignatureModifier => "Index signatures cannot have the '{}' modifier.",
	PrivateModifier => "Private elements cannot have the '{}' modifier.",
	StaticBlockModifier => "Static class blocks cannot have any modifier.",
	AccessorTypeParameters => "An accessor cannot have type parameters.",
	ConstructorTypeParameters => "Type parameters cannot appear on a constructor declaration.",
	SetterReturnType => "A 'set' accessor cannot have a return type annotation.",
	DecoratorPlacement => "Decorators must be attached to a class element.",
	DecoratorOnConstructor => "Decorators can't be used with a constructor. Did you mean '@dec class { ... }'?",
	ImplementationInAmbient => "An implementation cannot be declared in ambient contexts.",
	InitializerInAmbient => "Initializers are not allowed in ambient contexts.",
	AmbientConstInitializer => "A 'const' initializer in an ambient context must be a string or numeric literal or literal enum reference.",
	ExportDeclareWithoutDeclaration => "'export declare' must be followed by an ambient declaration.",
	InterfaceWithoutName => "'interface' declarations must be followed by an identifier.",
	TypeRedeclaration => "type '{}' has already been declared.",
	ImportTypeAlias => "An import alias can not use 'import type'.",
	TypeImportArgument => "Argument in a type import must be a string literal.",
	TypeImportDefaultAndNamed => "A type-only import can specify a default import or named bindings, but not both.",
	TypeModifierInTypeImport => "The 'type' modifier cannot be used on a named import when 'import type' is used on its import statement.",
	UnexpectedTypeAnnotation => "Did not expect a type annotation here.",
	TypeAnnotationAfterDefault => "Type annotations must come before default assignments, e.g. instead of `age = 25: number` use `age: number = 25`.",
	EmptyTypeArguments => "Type argument list cannot be empty.",
	EmptyTypeParameters => "Type parameter list cannot be empty.",
	EmptyList => "'{}' list cannot be empty.",
	TypeParameterModifier => "'{}' modifier cannot appear on a type parameter.",
	InvalidConst => "Cannot find name 'const'.",
	TupleLabel => "Tuple members must be labeled with a simple identifier.",
	RequiredAfterOptional => "A required element cannot follow an optional element.",
	OptionalPatternParameter => "A binding pattern parameter cannot be optional in an implementation signature.",
	ParameterPropertyPattern => "A parameter property may not be declared using a binding pattern.",
	SignatureParameterDefault => "Name in a signature must be an Identifier, ObjectPattern or ArrayPattern, instead got AssignmentPattern.",
	InvalidRequest => "{}",
	PropertyAfterInstantiation => "Invalid property access after an instantiation expression. You can either wrap the instantiation expression in parentheses, or delete the type arguments.",
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
	pub code: Code,
	pub message: String,
	/// Byte offset of the error, and of the end of the offending token when there is one.
	pub pos: u32,
	pub end: u32,
}

impl SyntaxError {
	pub fn new(pos: u32, code: Code) -> Self {
		Self::with(pos, code, code.message())
	}

	pub fn with(pos: u32, code: Code, message: impl Into<String>) -> Self {
		Self {
			code,
			message: message.into(),
			pos,
			end: pos,
		}
	}

	pub fn to(mut self, end: u32) -> Self {
		self.end = end;
		self
	}
}

impl fmt::Display for SyntaxError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} ({})", self.message, self.pos)
	}
}

impl std::error::Error for SyntaxError {}
