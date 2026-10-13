use crate::interner::StrId;

macro_rules! codes {
	($($(#[$meta:meta])* $name:ident,)*) => {
		/// What went wrong, as the byte the engine reports; what each means is on its variant, with
		/// what its `args` hold. The JavaScript side names each in `Code`, in this order.
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
		}
	};
}

codes! {
	/// A token the grammar does not allow here.
	UnexpectedToken,
	/// A token other than the one the grammar needs next; `args` names what was needed.
	Expected,
	/// A tag, block or bracket never closed; `args` names it.
	Unclosed,
	/// A closing tag, block or bracket with nothing open; `args` names it.
	UnexpectedClose,
	/// A name a host's grammar cannot take; `args` names it.
	InvalidName,
	/// A thing that may appear once appears again; `args` names it.
	Duplicate,
	/// A construct in a place the grammar does not allow it; `args` names it.
	Placement,
	/// The text ended while a token was still needed; `pos` is the end of what was parsed.
	UnexpectedEof,
	/// A character no token starts with; `args` holds it.
	UnexpectedCharacter,
	/// A keyword where a name or expression is needed; `args` names it.
	UnexpectedKeyword,
	/// A reserved word used as a name; `args` names it.
	ReservedWord,
	/// A keyword spelled with an escape sequence; `args` names it.
	EscapeInKeyword,
	/// Nesting deeper than the engine's limit.
	NestingDepth,
	/// A tree larger than the engine's limit.
	TreeSize,
	/// TypeScript syntax in a parse without `typescript`.
	NotTypescript,
	/// A comment the text ends inside.
	UnterminatedComment,
	/// A string the line or text ends inside.
	UnterminatedString,
	/// A template the text ends inside.
	UnterminatedTemplate,
	/// A regular expression the line or text ends inside.
	UnterminatedRegexp,
	/// A regular expression pattern that does not parse; `args` holds the pattern and the reason.
	InvalidRegexp,
	/// A regular expression flag that does not exist.
	InvalidRegexpFlag,
	/// A regular expression flag given twice.
	DuplicateRegexpFlag,
	/// A number that does not parse.
	InvalidNumber,
	/// A radix prefix with no digits after it; `args` holds the radix.
	ExpectedNumberInRadix,
	/// An identifier starting right after a number.
	IdentifierAfterNumber,
	/// A numeric separator before the first digit.
	NumericSeparatorFirst,
	/// A numeric separator after the last digit.
	NumericSeparatorLast,
	/// Two numeric separators in a row.
	NumericSeparatorDouble,
	/// A numeric separator in a legacy octal number.
	NumericSeparatorLegacyOctal,
	/// An escape sequence that does not parse.
	BadCharacterEscape,
	/// An escape sequence that does not parse, in a template with no tag.
	BadTemplateEscape,
	/// A `\u` escape that does not parse.
	InvalidUnicodeEscape,
	/// A `\` in a name with no `u` after it.
	ExpectedUnicodeEscape,
	/// A `\u{…}` escape above the last code point.
	CodePointOutOfBounds,
	/// `with` in strict mode.
	StrictWith,
	/// `delete` of a name in strict mode.
	StrictDelete,
	/// `"use strict"` in a function whose parameters are not all plain names.
	StrictDirectiveNonSimpleParams,
	/// A name strict mode does not allow to be declared or assigned; `args` names it.
	StrictBinding,
	/// A legacy octal number or escape in strict mode.
	StrictOctal,
	/// A `\8` or `\9` escape in strict mode.
	StrictEscape,
	/// `let` as the name of a `let`, `const`, `using` or class declaration.
	LetAsBinding,
	/// A name declared again where it cannot be; `args` names it.
	Redeclaration,
	/// A label already in use; `args` names it.
	DuplicateLabel,
	/// A parameter name used twice where that is not allowed.
	DuplicateParameter,
	/// `__proto__` twice in an object literal.
	DuplicateProto,
	/// Two `default` clauses in a `switch`.
	DuplicateDefault,
	/// A name exported twice; `args` names it.
	DuplicateExport,
	/// An exported name that is not declared; `args` names it.
	UndefinedExport,
	/// An export of a string name with no `from`.
	StringExportWithoutFrom,
	/// A lone surrogate in a string export name.
	LoneSurrogateInModuleName,
	/// An import attribute key given twice; `args` names it.
	DuplicateImportAttribute,
	/// `import` or `export` with `sourceType` `script`.
	ImportExportInScript,
	/// `import` or `export` below the top level.
	ImportExportNotTopLevel,
	/// `import.meta` outside a module.
	ImportMetaOutsideModule,
	/// `import.meta` spelled with an escape sequence.
	ImportMetaEscaped,
	/// `import.` followed by something other than `meta`.
	InvalidImportMeta,
	/// `new.` followed by something other than `target`.
	InvalidNewTarget,
	/// `new.target` spelled with an escape sequence.
	NewTargetEscaped,
	/// `new.target` outside a function or class static block.
	NewTargetOutsideFunction,
	/// A `break` or `continue` with no loop, switch or label to go to; `args` names the statement.
	Unsyntactic,
	/// `return` outside a function.
	ReturnOutsideFunction,
	/// A line break between `throw` and its expression.
	NewlineAfterThrow,
	/// A `try` with neither `catch` nor `finally`.
	MissingCatchOrFinally,
	/// An initializer on the declaration of a `for-in` or `for-of`; `args` names the loop.
	ForInOfInitializer,
	/// A `for-of` whose left side starts with `let`.
	ForOfLet,
	/// `await` as a name inside an async function.
	AwaitAsIdentifier,
	/// `await` outside an async function or module.
	AwaitOutsideAsync,
	/// `await` in a parameter's default value.
	AwaitInDefaultValue,
	/// `yield` as a name inside a generator.
	YieldAsIdentifier,
	/// `yield` in a parameter's default value.
	YieldInDefaultValue,
	/// `await` or `arguments` in a class static block; `args` names it.
	InvalidInStaticBlock,
	/// `arguments` in a class field initializer.
	ArgumentsInFieldInitializer,
	/// An assignment to something that cannot be assigned.
	InvalidAssignmentTarget,
	/// A binding to something that cannot be bound.
	InvalidBindingTarget,
	/// A member expression where a binding is needed.
	BindingMemberExpression,
	/// A parenthesized pattern.
	ParenthesizedPattern,
	/// A parenthesized expression where a binding is needed.
	BindingParenthesized,
	/// An optional chain assigned to.
	OptionalChainAssignment,
	/// An optional chain as the callee of `new`.
	OptionalChainInNew,
	/// An optional chain as the tag of a template.
	OptionalChainInTaggedTemplate,
	/// `??` mixed with `||` or `&&` without parentheses.
	MixedCoalesce,
	/// A private name outside the left side of `in`.
	PrivateNameOutsideIn,
	/// A private name no enclosing class declares; `args` names it.
	UndeclaredPrivateName,
	/// `delete` of a private name.
	DeletePrivate,
	/// `super` where neither a call nor a property follows.
	InvalidSuper,
	/// `super` outside a method.
	SuperOutsideMethod,
	/// `super()` outside the constructor of a derived class.
	SuperCallOutsideConstructor,
	/// A comma after a rest element.
	CommaAfterRest,
	/// A default value on a rest element.
	RestWithDefault,
	/// A getter or setter in an object pattern.
	AccessorInPattern,
	/// A shorthand `a = 1` in an object that is not a pattern.
	ShorthandAssignment,
	/// A pattern declared without a value.
	PatternWithoutInitializer,
	/// A pattern in a `using` declaration; `args` names the declaration.
	UsingPattern,
	/// A `using` declaration without a value; `args` names the declaration.
	UsingWithoutInitializer,
	/// A `using` declaration as the left side of a `for-in`; `args` names the declaration.
	UsingInForIn,
	/// A `using` declaration at the top level of a script or in a case clause; `args` names the declaration.
	UsingOutsideBlock,
	/// An operator other than `=` giving a default value.
	InvalidDefaultOperator,
	/// A getter with parameters.
	GetterParams,
	/// A setter with other than one parameter.
	SetterParams,
	/// A setter with a rest parameter.
	SetterRestParam,
	/// A constructor that is a generator.
	GeneratorConstructor,
	/// A constructor that is async.
	AsyncConstructor,
	/// A constructor that is a getter or setter.
	AccessorConstructor,
	/// Two constructors in a class.
	DuplicateConstructor,
	/// A class field named `constructor`.
	ConstructorField,
	/// A class element named `#constructor`.
	PrivateConstructor,
	/// A static class element named `prototype`.
	StaticPrototype,
	/// An abstract method outside an abstract class.
	AbstractOutsideAbstractClass,
	/// An abstract method with a body; `args` names it.
	AbstractWithImplementation,
	/// An abstract property with a value; `args` names it.
	AbstractWithInitializer,
	/// Two accessibility modifiers on one element.
	DuplicateAccessibility,
	/// A modifier given twice; `args` names it.
	DuplicateModifier,
	/// Two modifiers that cannot go together; `args` names both.
	ConflictingModifiers,
	/// Two modifiers in the wrong order; `args` names the one that must come first, then the other.
	ModifierOrder,
	/// `declare` on a method.
	DeclareOnMethod,
	/// `readonly` on a method.
	ReadonlyOnMethod,
	/// `readonly` on other than a property or index signature.
	ReadonlyPlacement,
	/// `readonly` on a type other than an array or tuple.
	ReadonlyTypeOperand,
	/// `override` in a class that extends nothing.
	OverrideWithoutExtends,
	/// A modifier on an index signature; `args` names it.
	IndexSignatureModifier,
	/// A modifier on a private element; `args` names it.
	PrivateModifier,
	/// A modifier on a static block.
	StaticBlockModifier,
	/// Type parameters on a getter or setter.
	AccessorTypeParameters,
	/// Type parameters on a constructor.
	ConstructorTypeParameters,
	/// A return type on a setter.
	SetterReturnType,
	/// A decorator on something other than a class or a class element, or split around `export`.
	DecoratorPlacement,
	/// A decorator on a constructor.
	DecoratorOnConstructor,
	/// A decorator on a method or accessor without a body.
	DecoratorWithoutBody,
	/// A decorator or modifier on a `this` parameter.
	ThisParameterModifiers,
	/// A parameter both optional and with a default value.
	OptionalWithInitializer,
	/// A required parameter or tuple element after an optional one.
	RequiredAfterOptional,
	/// A rest parameter marked optional.
	OptionalRest,
	/// An optional parameter on a setter.
	SetterOptionalParameter,
	/// A default value on a setter's parameter.
	SetterParameterInitializer,
	/// A return type on a constructor.
	ConstructorReturnType,
	/// A modifier on a constructor; `args` names it.
	ConstructorModifier,
	/// An index signature without a type.
	IndexSignatureType,
	/// A type predicate outside a return type.
	PredicatePlacement,
	/// `infer` outside the `extends` clause of a conditional type.
	InferPlacement,
	/// `as const` on other than a literal or a reference.
	ConstAssertionTarget,
	/// A catch clause variable typed other than `any` or `unknown`.
	CatchClauseType,
	/// `abstract` on other than a class, method or property.
	AbstractPlacement,
	/// `export =` in a namespace.
	ExportAssignmentInNamespace,
	/// A quoted module name on a module that is not ambient.
	QuotedModuleName,
	/// A body in an ambient context.
	ImplementationInAmbient,
	/// A value in an ambient context.
	InitializerInAmbient,
	/// An ambient `const` with a value other than a literal or enum member.
	AmbientConstInitializer,
	/// `export declare` followed by other than a declaration.
	ExportDeclareWithoutDeclaration,
	/// `interface` followed by other than a name.
	InterfaceWithoutName,
	/// A type declared again; `args` names it.
	TypeRedeclaration,
	/// `import type` on an import alias.
	ImportTypeAlias,
	/// A type import of other than a string.
	TypeImportArgument,
	/// `import type` with both a default and named imports.
	TypeImportDefaultAndNamed,
	/// `type` on a name of an `import type` or `export type`.
	TypeModifierInTypeImport,
	/// A type annotation where none can be.
	UnexpectedTypeAnnotation,
	/// A type annotation after a default value.
	TypeAnnotationAfterDefault,
	/// `<>` with no type arguments.
	EmptyTypeArguments,
	/// `<>` with no type parameters.
	EmptyTypeParameters,
	/// A list that cannot be empty; `args` names it.
	EmptyList,
	/// A modifier on a type member; `args` names it.
	TypeMemberModifier,
	/// A modifier on a type parameter where it cannot be; `args` names it.
	TypeParameterModifier,
	/// `const` as a type.
	InvalidConst,
	/// A tuple label that is not a plain name.
	TupleLabel,
	/// An optional pattern parameter in an implementation.
	OptionalPatternParameter,
	/// A parameter property declared with a pattern.
	ParameterPropertyPattern,
	/// A default value on a parameter of a signature.
	SignatureParameterDefault,
	/// A `!` definite assignment on a declaration with a value.
	DefiniteWithInitializer,
	/// A generator in an ambient context.
	GeneratorInAmbient,
	/// An overload signature marked as a generator.
	GeneratorSignature,
	/// `declare` inside an ambient context.
	DeclareInAmbient,
	/// A statement in an ambient context.
	StatementInAmbient,
	/// A `using` declaration in an ambient context; `args` names the declaration.
	UsingInAmbient,
	/// An offset or option the request cannot take; `args` says which, no `line` or `column`.
	InvalidRequest,
	/// A property access right after an instantiation expression.
	PropertyAfterInstantiation,
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
		let mut out = self.code.name();
		for &arg in self.args.iter().flatten() {
			out.push_str(" '");
			out.push_str(strings.get(arg));
			out.push('\'');
		}
		out
	}
}

#[cfg(test)]
impl Code {
	/// The name the JavaScript side gives this byte: the variant in snake case, `unexpected_eof`.
	pub(crate) fn name(self) -> String {
		let mut out = String::new();
		for c in format!("{self:?}").chars() {
			if c.is_ascii_uppercase() && !out.is_empty() {
				out.push('_');
			}
			out.push(c.to_ascii_lowercase());
		}
		out
	}
}

#[cfg(test)]
mod tests {
	use super::Code;

	// the JavaScript side names every code by hand, in this order; a code added, renamed, moved
	// or dropped here fails until types.ts says the same
	#[test]
	fn types_ts_names_every_code() {
		let ts = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../npm/src/types.ts")).unwrap();
		let list: String = Code::ALL.iter().map(|code| format!("\n\t'{}',", code.name())).collect();
		let list = format!("export const CODES = [{list}\n] as const;\n");
		assert!(
			ts.contains(&list),
			"npm/src/types.ts `CODES` disagrees with error.rs; it reads\n{list}"
		);
	}
}
