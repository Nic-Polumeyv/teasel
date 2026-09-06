use std::fmt;

macro_rules! codes {
	($($name:ident $code:literal => $message:literal,)*) => {
		/// What went wrong, as a stable name for hosts to branch on; the message is for people.
		#[derive(Clone, Copy, Debug, PartialEq, Eq)]
		#[non_exhaustive]
		pub enum Code {
			$($name,)*
		}

		impl Code {
			/// The name hosts see: `unexpected_eof`.
			pub fn name(self) -> &'static str {
				match self {
					$(Code::$name => $code,)*
				}
			}

			/// The message with its `{}` placeholders, when it has any.
			pub fn message(self) -> &'static str {
				match self {
					$(Code::$name => $message,)*
				}
			}

			/// The message with its one placeholder filled.
			pub fn with(self, arg: &str) -> String {
				self.message().replacen("{}", arg, 1)
			}
		}
	};
}

codes! {
	UnexpectedToken "unexpected_token" => "Unexpected token",
	UnexpectedEof "unexpected_eof" => "Unexpected end of input",
	UnexpectedCharacter "unexpected_character" => "Unexpected character '{}'",
	UnexpectedKeyword "unexpected_keyword" => "Unexpected keyword '{}'",
	ReservedWord "reserved_word" => "The keyword '{}' is reserved",
	EscapeInKeyword "escape_in_keyword" => "Escape sequence in keyword {}",
	NestingDepth "nesting_depth" => "Maximum nesting depth exceeded",
	UnterminatedComment "unterminated_comment" => "Unterminated comment",
	UnterminatedString "unterminated_string" => "Unterminated string constant",
	UnterminatedTemplate "unterminated_template" => "Unterminated template",
	UnterminatedRegexp "unterminated_regexp" => "Unterminated regular expression",
	InvalidRegexp "invalid_regexp" => "Invalid regular expression: /{}/: {}",
	InvalidRegexpFlag "invalid_regexp_flag" => "Invalid regular expression flag",
	DuplicateRegexpFlag "duplicate_regexp_flag" => "Duplicate regular expression flag",
	InvalidNumber "invalid_number" => "Invalid number",
	ExpectedNumberInRadix "expected_number_in_radix" => "Expected number in radix {}",
	IdentifierAfterNumber "identifier_after_number" => "Identifier directly after number",
	NumericSeparatorFirst "numeric_separator_first" => "Numeric separator is not allowed at the first of digits",
	NumericSeparatorLast "numeric_separator_last" => "Numeric separator is not allowed at the last of digits",
	NumericSeparatorDouble "numeric_separator_double" => "Numeric separator must be exactly one underscore",
	NumericSeparatorLegacyOctal "numeric_separator_legacy_octal" => "Numeric separator is not allowed in legacy octal numeric literals",
	BadCharacterEscape "bad_character_escape" => "Bad character escape sequence",
	BadTemplateEscape "bad_template_escape" => "Bad escape sequence in untagged template literal",
	InvalidUnicodeEscape "invalid_unicode_escape" => "Invalid Unicode escape",
	ExpectedUnicodeEscape "expected_unicode_escape" => "Expecting Unicode escape sequence \\uXXXX",
	CodePointOutOfBounds "code_point_out_of_bounds" => "Code point out of bounds",
	StrictWith "strict_with" => "'with' in strict mode",
	StrictDelete "strict_delete" => "Deleting local variable in strict mode",
	StrictDirectiveNonSimpleParams "strict_directive_non_simple_params" => "Illegal 'use strict' directive in function with non-simple parameter list",
	StrictBinding "strict_binding" => "{}{} in strict mode",
	StrictOctal "strict_octal" => "Octal literal in strict mode",
	StrictEscape "strict_escape" => "Invalid escape sequence",
	LetAsBinding "let_as_binding" => "let is disallowed as a lexically bound name",
	Redeclaration "redeclaration" => "Identifier '{}' has already been declared",
	DuplicateLabel "duplicate_label" => "Label '{}' is already declared",
	DuplicateParameter "duplicate_parameter" => "Argument name clash",
	DuplicateProto "duplicate_proto" => "Redefinition of __proto__ property",
	DuplicateDefault "duplicate_default" => "Multiple default clauses",
	DuplicateExport "duplicate_export" => "Duplicate export '{}'",
	UndefinedExport "undefined_export" => "Export '{}' is not defined",
	StringExportWithoutFrom "string_export_without_from" => "A string literal cannot be used as an exported binding without `from`.",
	DuplicateImportAttribute "duplicate_import_attribute" => "Duplicate attribute key '{}'",
	ImportExportInScript "import_export_in_script" => "'import' and 'export' may appear only with 'sourceType: module'",
	ImportExportNotTopLevel "import_export_not_top_level" => "'import' and 'export' may only appear at the top level",
	ImportMetaOutsideModule "import_meta_outside_module" => "Cannot use 'import.meta' outside a module",
	ImportMetaEscaped "import_meta_escaped" => "'import.meta' must not contain escaped characters",
	InvalidImportMeta "invalid_import_meta" => "The only valid meta property for import is 'import.meta'",
	InvalidNewTarget "invalid_new_target" => "The only valid meta property for new is 'new.target'",
	NewTargetEscaped "new_target_escaped" => "'new.target' must not contain escaped characters",
	NewTargetOutsideFunction "new_target_outside_function" => "'new.target' can only be used in functions and class static block",
	Unsyntactic "unsyntactic" => "Unsyntactic {}",
	ReturnOutsideFunction "return_outside_function" => "'return' outside of function",
	NewlineAfterThrow "newline_after_throw" => "Illegal newline after throw",
	MissingCatchOrFinally "missing_catch_or_finally" => "Missing catch or finally clause",
	ForInOfInitializer "for_in_of_initializer" => "{} loop variable declaration may not have an initializer",
	ForOfLet "for_of_let" => "The left-hand side of a for-of loop may not start with 'let'.",
	AwaitAsIdentifier "await_as_identifier" => "Cannot use 'await' as identifier inside an async function",
	AwaitOutsideAsync "await_outside_async" => "Cannot use keyword 'await' outside an async function",
	AwaitInDefaultValue "await_in_default_value" => "Await expression cannot be a default value",
	YieldAsIdentifier "yield_as_identifier" => "Cannot use 'yield' as identifier inside a generator",
	YieldInDefaultValue "yield_in_default_value" => "Yield expression cannot be a default value",
	InvalidInStaticBlock "invalid_in_static_block" => "Cannot use {} in class static initialization block",
	ArgumentsInFieldInitializer "arguments_in_field_initializer" => "Cannot use 'arguments' in class field initializer",
	InvalidAssignmentTarget "invalid_assignment_target" => "Assigning to rvalue",
	InvalidBindingTarget "invalid_binding_target" => "Binding rvalue",
	BindingMemberExpression "binding_member_expression" => "Binding member expression",
	ParenthesizedPattern "parenthesized_pattern" => "Parenthesized pattern",
	BindingParenthesized "binding_parenthesized" => "Binding parenthesized expression",
	OptionalChainAssignment "optional_chain_assignment" => "Optional chaining cannot appear in left-hand side",
	OptionalChainInNew "optional_chain_in_new" => "Optional chaining cannot appear in the callee of new expressions",
	OptionalChainInTaggedTemplate "optional_chain_in_tagged_template" => "Optional chaining cannot appear in the tag of tagged template expressions",
	MixedCoalesce "mixed_coalesce" => "Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses",
	PrivateNameOutsideIn "private_name_outside_in" => "Private identifier can only be left side of binary expression",
	UndeclaredPrivateName "undeclared_private_name" => "Private field '#{}' must be declared in an enclosing class",
	DeletePrivate "delete_private" => "Private fields can not be deleted",
	InvalidSuper "invalid_super" => "Invalid use of 'super'",
	SuperOutsideMethod "super_outside_method" => "'super' keyword outside a method",
	SuperCallOutsideConstructor "super_call_outside_constructor" => "super() call outside constructor of a subclass",
	CommaAfterRest "comma_after_rest" => "Comma is not permitted after the rest element",
	RestWithDefault "rest_with_default" => "Rest elements cannot have a default value",
	AccessorInPattern "accessor_in_pattern" => "Object pattern can't contain getter or setter",
	ShorthandAssignment "shorthand_assignment" => "Shorthand property assignments are valid only in destructuring patterns",
	PatternWithoutInitializer "pattern_without_initializer" => "Complex binding patterns require an initialization value",
	InvalidDefaultOperator "invalid_default_operator" => "Only '=' operator can be used for specifying default value.",
	GetterParams "getter_params" => "getter should have no params",
	SetterParams "setter_params" => "setter should have exactly one param",
	SetterRestParam "setter_rest_param" => "Setter cannot use rest params",
	GeneratorConstructor "generator_constructor" => "Constructor can't be a generator",
	AsyncConstructor "async_constructor" => "Constructor can't be an async method",
	AccessorConstructor "accessor_constructor" => "Constructor can't have get/set modifier",
	DuplicateConstructor "duplicate_constructor" => "Duplicate constructor in the same class",
	ConstructorField "constructor_field" => "Classes can't have a field named 'constructor'",
	PrivateConstructor "private_constructor" => "Classes can't have an element named '#constructor'",
	StaticPrototype "static_prototype" => "Classes can't have a static field named 'prototype'",
	AbstractOutsideAbstractClass "abstract_outside_abstract_class" => "Abstract methods can only appear within an abstract class.",
	AbstractWithImplementation "abstract_with_implementation" => "Method '{}' cannot have an implementation because it is marked abstract.",
	AbstractWithInitializer "abstract_with_initializer" => "Property '{}' cannot have an initializer because it is marked abstract.",
	DuplicateAccessibility "duplicate_accessibility" => "Accessibility modifier already seen.",
	DuplicateModifier "duplicate_modifier" => "Duplicate modifier: '{}'.",
	ConflictingModifiers "conflicting_modifiers" => "'{}' modifier cannot be used with '{}' modifier.",
	ModifierOrder "modifier_order" => "'{}' modifier must precede '{}' modifier.",
	DeclareOnMethod "declare_on_method" => "Class methods cannot have the 'declare' modifier.",
	ReadonlyOnMethod "readonly_on_method" => "Class methods cannot have the 'readonly' modifier.",
	ReadonlyPlacement "readonly_placement" => "'readonly' modifier can only appear on a property declaration or index signature.",
	ReadonlyTypeOperand "readonly_type_operand" => "'readonly' type modifier is only permitted on array and tuple literal types.",
	OverrideWithoutExtends "override_without_extends" => "This member cannot have an 'override' modifier because its containing class does not extend another class.",
	IndexSignatureModifier "index_signature_modifier" => "Index signatures cannot have the '{}' modifier.",
	PrivateModifier "private_modifier" => "Private elements cannot have the '{}' modifier.",
	StaticBlockModifier "static_block_modifier" => "Static class blocks cannot have any modifier.",
	AccessorTypeParameters "accessor_type_parameters" => "An accessor cannot have type parameters.",
	ConstructorTypeParameters "constructor_type_parameters" => "Type parameters cannot appear on a constructor declaration.",
	SetterReturnType "setter_return_type" => "A 'set' accessor cannot have a return type annotation.",
	DecoratorPlacement "decorator_placement" => "Decorators must be attached to a class element.",
	DecoratorOnConstructor "decorator_on_constructor" => "Decorators can't be used with a constructor. Did you mean '@dec class { ... }'?",
	ImplementationInAmbient "implementation_in_ambient" => "An implementation cannot be declared in ambient contexts.",
	InitializerInAmbient "initializer_in_ambient" => "Initializers are not allowed in ambient contexts.",
	AmbientConstInitializer "ambient_const_initializer" => "A 'const' initializer in an ambient context must be a string or numeric literal or literal enum reference.",
	ExportDeclareWithoutDeclaration "export_declare_without_declaration" => "'export declare' must be followed by an ambient declaration.",
	InterfaceWithoutName "interface_without_name" => "'interface' declarations must be followed by an identifier.",
	TypeRedeclaration "type_redeclaration" => "type '{}' has already been declared.",
	ImportTypeAlias "import_type_alias" => "An import alias can not use 'import type'.",
	TypeImportArgument "type_import_argument" => "Argument in a type import must be a string literal.",
	TypeImportDefaultAndNamed "type_import_default_and_named" => "A type-only import can specify a default import or named bindings, but not both.",
	TypeModifierInTypeImport "type_modifier_in_type_import" => "The 'type' modifier cannot be used on a named import when 'import type' is used on its import statement.",
	UnexpectedTypeAnnotation "unexpected_type_annotation" => "Did not expect a type annotation here.",
	TypeAnnotationAfterDefault "type_annotation_after_default" => "Type annotations must come before default assignments, e.g. instead of `age = 25: number` use `age: number = 25`.",
	EmptyTypeArguments "empty_type_arguments" => "Type argument list cannot be empty.",
	EmptyTypeParameters "empty_type_parameters" => "Type parameter list cannot be empty.",
	EmptyList "empty_list" => "'{}' list cannot be empty.",
	TypeMemberModifier "type_member_modifier" => "'{}' modifier cannot appear on a type member.",
	TypeParameterModifier "type_parameter_modifier" => "'{}' modifier cannot appear on a type parameter.",
	InvalidConst "invalid_const" => "Cannot find name 'const'.",
	TupleLabel "tuple_label" => "Tuple members must be labeled with a simple identifier.",
	RequiredAfterOptional "required_after_optional" => "A required element cannot follow an optional element.",
	OptionalPatternParameter "optional_pattern_parameter" => "A binding pattern parameter cannot be optional in an implementation signature.",
	ParameterPropertyPattern "parameter_property_pattern" => "A parameter property may not be declared using a binding pattern.",
	SignatureParameterDefault "signature_parameter_default" => "Name in a signature must be an Identifier, ObjectPattern or ArrayPattern, instead got AssignmentPattern.",
	// A host's offsets or switches, with the message given where it is raised.
	InvalidRequest "invalid_request" => "",
	PropertyAfterInstantiation "property_after_instantiation" => "Invalid property access after an instantiation expression. You can either wrap the instantiation expression in parentheses, or delete the type arguments.",
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
