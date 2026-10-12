use crate::interner::StrId;

macro_rules! codes {
	($($(#[$meta:meta])* $name:ident $code:literal,)*) => {
		/// What went wrong, by name; what each means is on its variant, with what its `args` hold.
		#[derive(Clone, Copy, Debug, PartialEq, Eq)]
		#[non_exhaustive]
		#[repr(u8)]
		pub enum Code {
			$($(#[$meta])* $name,)*
		}

		impl Code {
			pub const ALL: &[Code] = &[$(Code::$name,)*];

			pub(crate) fn is_limit(self) -> bool {
				matches!(self, Code::NestingDepth | Code::TreeSize)
			}

			/// The name hosts see: `unexpected_eof`.
			pub fn name(self) -> &'static str {
				match self {
					$(Code::$name => $code,)*
				}
			}

			/// The name as the writer emits it.
			pub fn label(self) -> crate::names::Name {
				match self {
					$(Code::$name => crate::names::c!($code),)*
				}
			}
		}
	};
}

codes! {
	/// A token the grammar does not allow here.
	UnexpectedToken "unexpected_token",
	/// A token other than the one the grammar needs next; `args` names what was needed.
	Expected "expected",
	/// A tag, block or bracket never closed; `args` names it.
	Unclosed "unclosed",
	/// A closing tag, block or bracket with nothing open; `args` names it.
	UnexpectedClose "unexpected_close",
	/// A name a host's grammar cannot take; `args` names it.
	InvalidName "invalid_name",
	/// A thing that may appear once appears again; `args` names it.
	Duplicate "duplicate",
	/// A construct in a place the grammar does not allow it; `args` names it.
	Placement "placement",
	/// The text ended while a token was still needed; `pos` is the end of what was parsed.
	UnexpectedEof "unexpected_eof",
	/// A character no token starts with; `args` holds it.
	UnexpectedCharacter "unexpected_character",
	/// A keyword where a name or expression is needed; `args` names it.
	UnexpectedKeyword "unexpected_keyword",
	/// A reserved word used as a name; `args` names it.
	ReservedWord "reserved_word",
	/// A keyword spelled with an escape sequence; `args` names it.
	EscapeInKeyword "escape_in_keyword",
	/// Nesting deeper than the engine's limit.
	NestingDepth "nesting_depth",
	/// A tree larger than the engine's limit.
	TreeSize "tree_size",
	/// TypeScript syntax in a parse without `typescript`.
	NotTypeScript "not_typescript",
	/// A comment the text ends inside.
	UnterminatedComment "unterminated_comment",
	/// A string the line or text ends inside.
	UnterminatedString "unterminated_string",
	/// A template the text ends inside.
	UnterminatedTemplate "unterminated_template",
	/// A regular expression the line or text ends inside.
	UnterminatedRegexp "unterminated_regexp",
	/// A regular expression pattern that does not parse; `args` holds the pattern and the reason.
	InvalidRegexp "invalid_regexp",
	/// A regular expression flag that does not exist.
	InvalidRegexpFlag "invalid_regexp_flag",
	/// A regular expression flag given twice.
	DuplicateRegexpFlag "duplicate_regexp_flag",
	/// A number that does not parse.
	InvalidNumber "invalid_number",
	/// A radix prefix with no digits after it; `args` holds the radix.
	ExpectedNumberInRadix "expected_number_in_radix",
	/// An identifier starting right after a number.
	IdentifierAfterNumber "identifier_after_number",
	/// A numeric separator before the first digit.
	NumericSeparatorFirst "numeric_separator_first",
	/// A numeric separator after the last digit.
	NumericSeparatorLast "numeric_separator_last",
	/// Two numeric separators in a row.
	NumericSeparatorDouble "numeric_separator_double",
	/// A numeric separator in a legacy octal number.
	NumericSeparatorLegacyOctal "numeric_separator_legacy_octal",
	/// An escape sequence that does not parse.
	BadCharacterEscape "bad_character_escape",
	/// An escape sequence that does not parse, in a template with no tag.
	BadTemplateEscape "bad_template_escape",
	/// A `\u` escape that does not parse.
	InvalidUnicodeEscape "invalid_unicode_escape",
	/// A `\` in a name with no `u` after it.
	ExpectedUnicodeEscape "expected_unicode_escape",
	/// A `\u{…}` escape above the last code point.
	CodePointOutOfBounds "code_point_out_of_bounds",
	/// `with` in strict mode.
	StrictWith "strict_with",
	/// `delete` of a name in strict mode.
	StrictDelete "strict_delete",
	/// `"use strict"` in a function whose parameters are not all plain names.
	StrictDirectiveNonSimpleParams "strict_directive_non_simple_params",
	/// A name strict mode does not allow to be declared or assigned; `args` names it.
	StrictBinding "strict_binding",
	/// A legacy octal number or escape in strict mode.
	StrictOctal "strict_octal",
	/// A `\8` or `\9` escape in strict mode.
	StrictEscape "strict_escape",
	/// `let` as the name of a `let`, `const`, `using` or class declaration.
	LetAsBinding "let_as_binding",
	/// A name declared again where it cannot be; `args` names it.
	Redeclaration "redeclaration",
	/// A label already in use; `args` names it.
	DuplicateLabel "duplicate_label",
	/// A parameter name used twice where that is not allowed.
	DuplicateParameter "duplicate_parameter",
	/// `__proto__` twice in an object literal.
	DuplicateProto "duplicate_proto",
	/// Two `default` clauses in a `switch`.
	DuplicateDefault "duplicate_default",
	/// A name exported twice; `args` names it.
	DuplicateExport "duplicate_export",
	/// An exported name that is not declared; `args` names it.
	UndefinedExport "undefined_export",
	/// An export of a string name with no `from`.
	StringExportWithoutFrom "string_export_without_from",
	/// A lone surrogate in a string export name.
	LoneSurrogateInModuleName "lone_surrogate_in_module_name",
	/// An import attribute key given twice; `args` names it.
	DuplicateImportAttribute "duplicate_import_attribute",
	/// `import` or `export` with `sourceType` `script`.
	ImportExportInScript "import_export_in_script",
	/// `import` or `export` below the top level.
	ImportExportNotTopLevel "import_export_not_top_level",
	/// `import.meta` outside a module.
	ImportMetaOutsideModule "import_meta_outside_module",
	/// `import.meta` spelled with an escape sequence.
	ImportMetaEscaped "import_meta_escaped",
	/// `import.` followed by something other than `meta`.
	InvalidImportMeta "invalid_import_meta",
	/// `new.` followed by something other than `target`.
	InvalidNewTarget "invalid_new_target",
	/// `new.target` spelled with an escape sequence.
	NewTargetEscaped "new_target_escaped",
	/// `new.target` outside a function or class static block.
	NewTargetOutsideFunction "new_target_outside_function",
	/// A `break` or `continue` with no loop, switch or label to go to; `args` names the statement.
	Unsyntactic "unsyntactic",
	/// `return` outside a function.
	ReturnOutsideFunction "return_outside_function",
	/// A line break between `throw` and its expression.
	NewlineAfterThrow "newline_after_throw",
	/// A `try` with neither `catch` nor `finally`.
	MissingCatchOrFinally "missing_catch_or_finally",
	/// An initializer on the declaration of a `for-in` or `for-of`; `args` names the loop.
	ForInOfInitializer "for_in_of_initializer",
	/// A `for-of` whose left side starts with `let`.
	ForOfLet "for_of_let",
	/// `await` as a name inside an async function.
	AwaitAsIdentifier "await_as_identifier",
	/// `await` outside an async function or module.
	AwaitOutsideAsync "await_outside_async",
	/// `await` in a parameter's default value.
	AwaitInDefaultValue "await_in_default_value",
	/// `yield` as a name inside a generator.
	YieldAsIdentifier "yield_as_identifier",
	/// `yield` in a parameter's default value.
	YieldInDefaultValue "yield_in_default_value",
	/// `await` or `arguments` in a class static block; `args` names it.
	InvalidInStaticBlock "invalid_in_static_block",
	/// `arguments` in a class field initializer.
	ArgumentsInFieldInitializer "arguments_in_field_initializer",
	/// An assignment to something that cannot be assigned.
	InvalidAssignmentTarget "invalid_assignment_target",
	/// A binding to something that cannot be bound.
	InvalidBindingTarget "invalid_binding_target",
	/// A member expression where a binding is needed.
	BindingMemberExpression "binding_member_expression",
	/// A parenthesized pattern.
	ParenthesizedPattern "parenthesized_pattern",
	/// A parenthesized expression where a binding is needed.
	BindingParenthesized "binding_parenthesized",
	/// An optional chain assigned to.
	OptionalChainAssignment "optional_chain_assignment",
	/// An optional chain as the callee of `new`.
	OptionalChainInNew "optional_chain_in_new",
	/// An optional chain as the tag of a template.
	OptionalChainInTaggedTemplate "optional_chain_in_tagged_template",
	/// `??` mixed with `||` or `&&` without parentheses.
	MixedCoalesce "mixed_coalesce",
	/// A private name outside the left side of `in`.
	PrivateNameOutsideIn "private_name_outside_in",
	/// A private name no enclosing class declares; `args` names it.
	UndeclaredPrivateName "undeclared_private_name",
	/// `delete` of a private name.
	DeletePrivate "delete_private",
	/// `super` where neither a call nor a property follows.
	InvalidSuper "invalid_super",
	/// `super` outside a method.
	SuperOutsideMethod "super_outside_method",
	/// `super()` outside the constructor of a derived class.
	SuperCallOutsideConstructor "super_call_outside_constructor",
	/// A comma after a rest element.
	CommaAfterRest "comma_after_rest",
	/// A default value on a rest element.
	RestWithDefault "rest_with_default",
	/// A getter or setter in an object pattern.
	AccessorInPattern "accessor_in_pattern",
	/// A shorthand `a = 1` in an object that is not a pattern.
	ShorthandAssignment "shorthand_assignment",
	/// A pattern declared without a value.
	PatternWithoutInitializer "pattern_without_initializer",
	/// A pattern in a `using` declaration; `args` names the declaration.
	UsingPattern "using_pattern",
	/// A `using` declaration without a value; `args` names the declaration.
	UsingWithoutInitializer "using_without_initializer",
	/// A `using` declaration as the left side of a `for-in`; `args` names the declaration.
	UsingInForIn "using_in_for_in",
	/// A `using` declaration at the top level of a script or in a case clause; `args` names the declaration.
	UsingOutsideBlock "using_outside_block",
	/// An operator other than `=` giving a default value.
	InvalidDefaultOperator "invalid_default_operator",
	/// A getter with parameters.
	GetterParams "getter_params",
	/// A setter with other than one parameter.
	SetterParams "setter_params",
	/// A setter with a rest parameter.
	SetterRestParam "setter_rest_param",
	/// A constructor that is a generator.
	GeneratorConstructor "generator_constructor",
	/// A constructor that is async.
	AsyncConstructor "async_constructor",
	/// A constructor that is a getter or setter.
	AccessorConstructor "accessor_constructor",
	/// Two constructors in a class.
	DuplicateConstructor "duplicate_constructor",
	/// A class field named `constructor`.
	ConstructorField "constructor_field",
	/// A class element named `#constructor`.
	PrivateConstructor "private_constructor",
	/// A static class element named `prototype`.
	StaticPrototype "static_prototype",
	/// An abstract method outside an abstract class.
	AbstractOutsideAbstractClass "abstract_outside_abstract_class",
	/// An abstract method with a body; `args` names it.
	AbstractWithImplementation "abstract_with_implementation",
	/// An abstract property with a value; `args` names it.
	AbstractWithInitializer "abstract_with_initializer",
	/// Two accessibility modifiers on one element.
	DuplicateAccessibility "duplicate_accessibility",
	/// A modifier given twice; `args` names it.
	DuplicateModifier "duplicate_modifier",
	/// Two modifiers that cannot go together; `args` names both.
	ConflictingModifiers "conflicting_modifiers",
	/// Two modifiers in the wrong order; `args` names the one that must come first, then the other.
	ModifierOrder "modifier_order",
	/// `declare` on a method.
	DeclareOnMethod "declare_on_method",
	/// `readonly` on a method.
	ReadonlyOnMethod "readonly_on_method",
	/// `readonly` on other than a property or index signature.
	ReadonlyPlacement "readonly_placement",
	/// `readonly` on a type other than an array or tuple.
	ReadonlyTypeOperand "readonly_type_operand",
	/// `override` in a class that extends nothing.
	OverrideWithoutExtends "override_without_extends",
	/// A modifier on an index signature; `args` names it.
	IndexSignatureModifier "index_signature_modifier",
	/// A modifier on a private element; `args` names it.
	PrivateModifier "private_modifier",
	/// A modifier on a static block.
	StaticBlockModifier "static_block_modifier",
	/// Type parameters on a getter or setter.
	AccessorTypeParameters "accessor_type_parameters",
	/// Type parameters on a constructor.
	ConstructorTypeParameters "constructor_type_parameters",
	/// A return type on a setter.
	SetterReturnType "setter_return_type",
	/// A decorator on something other than a class or a class element, or split around `export`.
	DecoratorPlacement "decorator_placement",
	/// A decorator on a constructor.
	DecoratorOnConstructor "decorator_on_constructor",
	/// A decorator on a method or accessor without a body.
	DecoratorWithoutBody "decorator_without_body",
	/// A decorator or modifier on a `this` parameter.
	ThisParameterModifiers "this_parameter_modifiers",
	/// A parameter both optional and with a default value.
	OptionalWithInitializer "optional_with_initializer",
	/// A required parameter or tuple element after an optional one.
	RequiredAfterOptional "required_after_optional",
	/// A rest parameter marked optional.
	OptionalRest "optional_rest",
	/// An optional parameter on a setter.
	SetterOptionalParameter "setter_optional_parameter",
	/// A default value on a setter's parameter.
	SetterParameterInitializer "setter_parameter_initializer",
	/// A return type on a constructor.
	ConstructorReturnType "constructor_return_type",
	/// A modifier on a constructor; `args` names it.
	ConstructorModifier "constructor_modifier",
	/// An index signature without a type.
	IndexSignatureType "index_signature_type",
	/// A type predicate outside a return type.
	PredicatePlacement "predicate_placement",
	/// `infer` outside the `extends` clause of a conditional type.
	InferPlacement "infer_placement",
	/// `as const` on other than a literal or a reference.
	ConstAssertionTarget "const_assertion_target",
	/// A catch clause variable typed other than `any` or `unknown`.
	CatchClauseType "catch_clause_type",
	/// `abstract` on other than a class, method or property.
	AbstractPlacement "abstract_placement",
	/// `export =` in a namespace.
	ExportAssignmentInNamespace "export_assignment_in_namespace",
	/// A quoted module name on a module that is not ambient.
	QuotedModuleName "quoted_module_name",
	/// A body in an ambient context.
	ImplementationInAmbient "implementation_in_ambient",
	/// A value in an ambient context.
	InitializerInAmbient "initializer_in_ambient",
	/// An ambient `const` with a value other than a literal or enum member.
	AmbientConstInitializer "ambient_const_initializer",
	/// `export declare` followed by other than a declaration.
	ExportDeclareWithoutDeclaration "export_declare_without_declaration",
	/// `interface` followed by other than a name.
	InterfaceWithoutName "interface_without_name",
	/// A type declared again; `args` names it.
	TypeRedeclaration "type_redeclaration",
	/// `import type` on an import alias.
	ImportTypeAlias "import_type_alias",
	/// A type import of other than a string.
	TypeImportArgument "type_import_argument",
	/// `import type` with both a default and named imports.
	TypeImportDefaultAndNamed "type_import_default_and_named",
	/// `type` on a name of an `import type` or `export type`.
	TypeModifierInTypeImport "type_modifier_in_type_import",
	/// A type annotation where none can be.
	UnexpectedTypeAnnotation "unexpected_type_annotation",
	/// A type annotation after a default value.
	TypeAnnotationAfterDefault "type_annotation_after_default",
	/// `<>` with no type arguments.
	EmptyTypeArguments "empty_type_arguments",
	/// `<>` with no type parameters.
	EmptyTypeParameters "empty_type_parameters",
	/// A list that cannot be empty; `args` names it.
	EmptyList "empty_list",
	/// A modifier on a type member; `args` names it.
	TypeMemberModifier "type_member_modifier",
	/// A modifier on a type parameter where it cannot be; `args` names it.
	TypeParameterModifier "type_parameter_modifier",
	/// `const` as a type.
	InvalidConst "invalid_const",
	/// A tuple label that is not a plain name.
	TupleLabel "tuple_label",
	/// An optional pattern parameter in an implementation.
	OptionalPatternParameter "optional_pattern_parameter",
	/// A parameter property declared with a pattern.
	ParameterPropertyPattern "parameter_property_pattern",
	/// A default value on a parameter of a signature.
	SignatureParameterDefault "signature_parameter_default",
	/// A `!` definite assignment on a declaration with a value.
	DefiniteWithInitializer "definite_with_initializer",
	/// A generator in an ambient context.
	GeneratorInAmbient "generator_in_ambient",
	/// An overload signature marked as a generator.
	GeneratorSignature "generator_signature",
	/// `declare` inside an ambient context.
	DeclareInAmbient "declare_in_ambient",
	/// A statement in an ambient context.
	StatementInAmbient "statement_in_ambient",
	/// A `using` declaration in an ambient context; `args` names the declaration.
	UsingInAmbient "using_in_ambient",
	/// An offset or option the request cannot take; `args` says which, no `loc`.
	InvalidRequest "invalid_request",
	/// A property access right after an instantiation expression.
	PropertyAfterInstantiation "property_after_instantiation",
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntaxError {
	pub code: Code,
	/// What the code is about, as strings of the parse that recorded the error: a name, a token.
	pub args: [Option<StrId>; 2],
	/// Byte offset of the error, and of the end of the offending token when there is one.
	pub pos: u32,
	pub end: u32,
}

impl SyntaxError {
	pub fn new(pos: u32, code: Code) -> Self {
		Self {
			code,
			args: [None; 2],
			pos,
			end: pos,
		}
	}

	pub fn arg(mut self, arg: StrId) -> Self {
		let free = self
			.args
			.iter()
			.position(Option::is_none)
			.expect("two arguments at most");
		self.args[free] = Some(arg);
		self
	}

	pub fn to(mut self, end: u32) -> Self {
		self.end = end;
		self
	}

	/// The code and its arguments as the package's `message` spells them: `expected '}'`.
	#[cfg(test)]
	pub(crate) fn describe(&self, strings: &crate::interner::Interner) -> String {
		let mut out = String::from(self.code.name());
		for &arg in self.args.iter().flatten() {
			out.push_str(" '");
			out.push_str(strings.get(arg));
			out.push('\'');
		}
		out
	}
}

#[cfg(test)]
mod tests {
	use super::Code;

	// the JavaScript side names every code by hand in its `Code` type; a code added, renamed or
	// dropped here fails until types.ts says the same
	#[test]
	fn types_ts_names_every_code() {
		let ts = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../npm/src/types.ts")).unwrap();
		let mut names: Vec<&str> = Code::ALL.iter().map(|code| code.name()).collect();
		names.dedup();
		let union: String = names.iter().map(|name| format!("\n\t| '{name}'")).collect();
		let union = format!("export type Code ={union};\n");
		assert!(
			ts.contains(&union),
			"npm/src/types.ts `Code` disagrees with error.rs; it reads\n{union}"
		);
	}
}
