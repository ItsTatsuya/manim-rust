//! A small syntax highlighter for [`Code`](super::code::Code): a lexer per language
//! family and Pygments' color styles (Manim colors code with Pygments).
//!
//! It classifies tokens the way Pygments does for the cases that show in a listing
//! (keywords, names after `def`/`fn`/`class`, builtins, strings with escapes and
//! interpolation, numbers, comments, decorators, macros, preprocessor lines), with no
//! dependencies. Languages: Python, Rust, JavaScript, TypeScript, C, C++, C#, Java, Go,
//! Kotlin, Swift, JSON, Bash, SQL and plain text.

/// Token classes, named after Pygments' token types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TokenKind {
    /// Plain text and identifiers (`Text`).
    Text,
    /// Keyword (`Keyword`).
    Keyword,
    /// Import or module keyword (`Keyword.Namespace`).
    KeywordNamespace,
    /// Constant keyword such as `true` or `None` (`Keyword.Constant`).
    KeywordConstant,
    /// Primitive type keyword (`Keyword.Type`).
    KeywordType,
    /// Declaration keyword such as `let` or `fn` (`Keyword.Declaration`).
    KeywordDeclaration,
    /// Builtin function or type (`Name.Builtin`).
    Builtin,
    /// Pseudo-builtin such as `self` (`Name.Builtin.Pseudo`).
    BuiltinPseudo,
    /// Function name at its definition (`Name.Function`).
    Function,
    /// Class or type name at its definition (`Name.Class`).
    Class,
    /// Decorator or annotation (`Name.Decorator`).
    Decorator,
    /// Macro invocation, e.g. `name!` in Rust.
    Macro,
    /// Attribute (`Name.Attribute`).
    Attribute,
    /// String literal (`String`).
    String,
    /// Escape sequence inside a string (`String.Escape`).
    StringEscape,
    /// Interpolated expression inside a string (`String.Interpol`).
    StringInterpol,
    /// Documentation string (`String.Doc`).
    StringDoc,
    /// Numeric literal (`Number`).
    Number,
    /// Comment (`Comment`).
    Comment,
    /// Preprocessor line (`Comment.Preproc`).
    Preproc,
    /// Symbolic operator (`Operator`).
    Operator,
    /// Word operator such as `and` or `in` (`Operator.Word`).
    OperatorWord,
    /// Punctuation (`Punctuation`).
    Punctuation,
    /// Markup tag (`Name.Tag`).
    Tag,
    /// Variable (`Name.Variable`).
    Variable,
}

/// One lexed piece of the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// The token class.
    pub kind: TokenKind,
    /// The source text of the token.
    pub text: String,
}

struct Lang {
    names: &'static [&'static str],
    extensions: &'static [&'static str],
    keywords: &'static [&'static str],
    namespace: &'static [&'static str],
    constants: &'static [&'static str],
    types: &'static [&'static str],
    declarations: &'static [&'static str],
    builtins: &'static [&'static str],
    pseudo: &'static [&'static str],
    operator_words: &'static [&'static str],
    /// The identifier after one of these is a function name.
    def_words: &'static [&'static str],
    /// The identifier after one of these is a class (type) name.
    class_words: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static str,
    /// Python: `'''`/`"""` strings and `r`/`b`/`f`/`u` prefixes, f-string `{...}`.
    python_strings: bool,
    /// JS template strings `` `...${x}...` ``.
    template_strings: bool,
    /// C: `#include`, `#define` lines.
    hash_preproc: bool,
    /// Rust: `#[derive(...)]` attributes, `name!` macros, `'a` lifetimes, `r#"..."#`.
    rust: bool,
    decorators: bool,
    dollar_vars: bool,
    json_keys: bool,
    case_insensitive: bool,
    dot_is_operator: bool,
    colon_is_punctuation: bool,
    ident_dollar: bool,
}

const NONE: &[&str] = &[];

const BASE: Lang = Lang {
    names: NONE,
    extensions: NONE,
    keywords: NONE,
    namespace: NONE,
    constants: NONE,
    types: NONE,
    declarations: NONE,
    builtins: NONE,
    pseudo: NONE,
    operator_words: NONE,
    def_words: NONE,
    class_words: NONE,
    line_comments: NONE,
    block_comment: None,
    quotes: "",
    python_strings: false,
    template_strings: false,
    hash_preproc: false,
    rust: false,
    decorators: false,
    dollar_vars: false,
    json_keys: false,
    case_insensitive: false,
    dot_is_operator: false,
    colon_is_punctuation: false,
    ident_dollar: false,
};

const C_LIKE: Lang = Lang { line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"'", ..BASE };

const PYTHON: Lang = Lang {
    names: &["python", "py", "python3", "py3", "sage"],
    extensions: &["py", "pyw", "pyi"],
    keywords: &[
        "assert", "async", "await", "break", "continue", "del", "elif", "else", "except", "finally", "for", "global",
        "if", "lambda", "nonlocal", "pass", "raise", "return", "try", "while", "with", "yield", "def", "class", "as",
        "match", "case",
    ],
    namespace: &["import", "from"],
    constants: &["True", "False", "None"],
    builtins: &[
        "abs",
        "all",
        "any",
        "ascii",
        "bin",
        "bool",
        "breakpoint",
        "bytearray",
        "bytes",
        "callable",
        "chr",
        "classmethod",
        "compile",
        "complex",
        "delattr",
        "dict",
        "dir",
        "divmod",
        "enumerate",
        "eval",
        "exec",
        "filter",
        "float",
        "format",
        "frozenset",
        "getattr",
        "globals",
        "hasattr",
        "hash",
        "help",
        "hex",
        "id",
        "input",
        "int",
        "isinstance",
        "issubclass",
        "iter",
        "len",
        "list",
        "locals",
        "map",
        "max",
        "memoryview",
        "min",
        "next",
        "object",
        "oct",
        "open",
        "ord",
        "pow",
        "print",
        "property",
        "range",
        "repr",
        "reversed",
        "round",
        "set",
        "setattr",
        "slice",
        "sorted",
        "staticmethod",
        "str",
        "sum",
        "super",
        "tuple",
        "type",
        "vars",
        "zip",
        "__import__",
        "NotImplemented",
        "Ellipsis",
    ],
    pseudo: &["self", "cls"],
    operator_words: &["and", "or", "not", "in", "is"],
    def_words: &["def"],
    class_words: &["class"],
    line_comments: &["#"],
    quotes: "\"'",
    python_strings: true,
    decorators: true,
    dot_is_operator: true,
    colon_is_punctuation: true,
    ..BASE
};

const RUST: Lang = Lang {
    names: &["rust", "rs"],
    extensions: &["rs"],
    keywords: &[
        "as",
        "async",
        "await",
        "break",
        "continue",
        "dyn",
        "else",
        "enum",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "loop",
        "match",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "struct",
        "trait",
        "type",
        "unsafe",
        "where",
        "while",
        "union",
        "macro_rules",
    ],
    namespace: &["use", "mod", "crate", "extern", "super"],
    constants: &["true", "false"],
    types: &[
        "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "bool",
        "char", "str", "String", "Vec", "Option", "Result", "Box",
    ],
    declarations: &["let", "const", "static"],
    builtins: &["Some", "None", "Ok", "Err"],
    pseudo: &["self", "Self"],
    def_words: &["fn"],
    class_words: &["struct", "enum", "trait", "union", "type"],
    rust: true,
    ..C_LIKE
};

const JAVASCRIPT: Lang = Lang {
    names: &["javascript", "js", "jsx", "mjs", "cjs", "node"],
    extensions: &["js", "jsx", "mjs", "cjs"],
    keywords: &[
        "async",
        "await",
        "break",
        "case",
        "catch",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "export",
        "extends",
        "finally",
        "for",
        "from",
        "if",
        "import",
        "in",
        "instanceof",
        "new",
        "of",
        "return",
        "static",
        "switch",
        "throw",
        "try",
        "typeof",
        "void",
        "while",
        "with",
        "yield",
        "get",
        "set",
        "as",
    ],
    constants: &["true", "false", "null", "undefined", "NaN", "Infinity"],
    declarations: &["var", "let", "const", "function", "class"],
    builtins: &[
        "Array",
        "Boolean",
        "Date",
        "Error",
        "Function",
        "Math",
        "Number",
        "Object",
        "Promise",
        "Proxy",
        "Reflect",
        "RegExp",
        "Set",
        "Map",
        "String",
        "Symbol",
        "JSON",
        "console",
        "window",
        "document",
        "globalThis",
        "parseInt",
        "parseFloat",
        "isNaN",
        "isFinite",
        "require",
        "module",
        "exports",
    ],
    pseudo: &["this", "super"],
    def_words: &["function"],
    class_words: &["class"],
    quotes: "\"'`",
    template_strings: true,
    decorators: true,
    ident_dollar: true,
    ..C_LIKE
};

const TYPESCRIPT: Lang = Lang {
    names: &["typescript", "ts", "tsx", "mts"],
    extensions: &["ts", "tsx", "mts", "cts"],
    keywords: &[
        "async",
        "await",
        "break",
        "case",
        "catch",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "export",
        "extends",
        "finally",
        "for",
        "from",
        "if",
        "import",
        "in",
        "instanceof",
        "new",
        "of",
        "return",
        "static",
        "switch",
        "throw",
        "try",
        "typeof",
        "void",
        "while",
        "with",
        "yield",
        "get",
        "set",
        "as",
        "implements",
        "private",
        "protected",
        "public",
        "readonly",
        "abstract",
        "declare",
        "namespace",
        "keyof",
        "infer",
        "is",
        "satisfies",
    ],
    types: &["string", "number", "boolean", "any", "unknown", "never", "object", "bigint", "symbol"],
    declarations: &["var", "let", "const", "function", "class", "interface", "type", "enum"],
    class_words: &["class", "interface", "type", "enum"],
    ..JAVASCRIPT
};

const C: Lang = Lang {
    names: &["c", "h"],
    extensions: &["c", "h"],
    keywords: &[
        "auto", "break", "case", "const", "continue", "default", "do", "else", "enum", "extern", "for", "goto", "if",
        "inline", "register", "restrict", "return", "signed", "sizeof", "static", "struct", "switch", "typedef",
        "union", "unsigned", "volatile", "while",
    ],
    constants: &["NULL", "true", "false"],
    types: &[
        "char", "short", "int", "long", "float", "double", "void", "bool", "size_t", "ssize_t", "int8_t", "int16_t",
        "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "FILE",
    ],
    class_words: &["struct", "union", "enum"],
    hash_preproc: true,
    ..C_LIKE
};

const CPP: Lang = Lang {
    names: &["cpp", "c++", "cxx", "cc", "hpp"],
    extensions: &["cpp", "cxx", "cc", "hpp", "hxx", "hh"],
    keywords: &[
        "auto",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "constexpr",
        "continue",
        "decltype",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "explicit",
        "export",
        "extern",
        "for",
        "friend",
        "goto",
        "if",
        "inline",
        "mutable",
        "namespace",
        "new",
        "noexcept",
        "operator",
        "override",
        "private",
        "protected",
        "public",
        "register",
        "return",
        "signed",
        "sizeof",
        "static",
        "static_assert",
        "static_cast",
        "dynamic_cast",
        "reinterpret_cast",
        "const_cast",
        "struct",
        "switch",
        "template",
        "throw",
        "try",
        "typedef",
        "typename",
        "union",
        "unsigned",
        "using",
        "virtual",
        "volatile",
        "while",
        "co_await",
        "co_return",
        "co_yield",
        "concept",
        "requires",
    ],
    constants: &["NULL", "nullptr", "true", "false"],
    pseudo: &["this"],
    class_words: &["class", "struct", "union", "enum"],
    ..C
};

const JAVA: Lang = Lang {
    names: &["java"],
    extensions: &["java"],
    keywords: &[
        "abstract",
        "assert",
        "break",
        "case",
        "catch",
        "class",
        "continue",
        "default",
        "do",
        "else",
        "enum",
        "extends",
        "final",
        "finally",
        "for",
        "if",
        "implements",
        "instanceof",
        "interface",
        "native",
        "new",
        "private",
        "protected",
        "public",
        "return",
        "static",
        "strictfp",
        "switch",
        "synchronized",
        "throw",
        "throws",
        "transient",
        "try",
        "volatile",
        "while",
        "var",
        "record",
        "yield",
    ],
    namespace: &["import", "package"],
    constants: &["true", "false", "null"],
    types: &["boolean", "byte", "char", "double", "float", "int", "long", "short", "void", "String"],
    pseudo: &["this", "super"],
    class_words: &["class", "interface", "enum", "record"],
    decorators: true,
    ..C_LIKE
};

const CSHARP: Lang = Lang {
    names: &["csharp", "c#", "cs"],
    extensions: &["cs"],
    keywords: &[
        "abstract",
        "as",
        "async",
        "await",
        "base",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "default",
        "delegate",
        "do",
        "else",
        "enum",
        "event",
        "explicit",
        "extern",
        "finally",
        "fixed",
        "for",
        "foreach",
        "get",
        "goto",
        "if",
        "implicit",
        "in",
        "interface",
        "internal",
        "is",
        "lock",
        "new",
        "operator",
        "out",
        "override",
        "params",
        "private",
        "protected",
        "public",
        "readonly",
        "record",
        "ref",
        "return",
        "sealed",
        "set",
        "sizeof",
        "static",
        "struct",
        "switch",
        "throw",
        "try",
        "typeof",
        "unsafe",
        "var",
        "virtual",
        "void",
        "volatile",
        "where",
        "while",
        "yield",
    ],
    namespace: &["using", "namespace"],
    constants: &["true", "false", "null"],
    types: &[
        "bool", "byte", "char", "decimal", "double", "float", "int", "long", "object", "sbyte", "short", "string",
        "uint", "ulong", "ushort", "dynamic",
    ],
    pseudo: &["this"],
    class_words: &["class", "struct", "interface", "enum", "record"],
    ..C_LIKE
};

const GO: Lang = Lang {
    names: &["go", "golang"],
    extensions: &["go"],
    keywords: &[
        "break",
        "case",
        "chan",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "interface",
        "map",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "type",
    ],
    namespace: &["package", "import"],
    constants: &["true", "false", "nil", "iota"],
    types: &[
        "bool",
        "byte",
        "complex64",
        "complex128",
        "error",
        "float32",
        "float64",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "rune",
        "string",
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "uintptr",
        "any",
    ],
    declarations: &["var", "const"],
    builtins: &[
        "append", "cap", "close", "complex", "copy", "delete", "imag", "len", "make", "new", "panic", "print",
        "println", "real", "recover", "min", "max", "clear",
    ],
    def_words: &["func"],
    class_words: &["type"],
    quotes: "\"'`",
    ..C_LIKE
};

const KOTLIN: Lang = Lang {
    names: &["kotlin", "kt", "kts"],
    extensions: &["kt", "kts"],
    keywords: &[
        "as",
        "break",
        "class",
        "continue",
        "do",
        "else",
        "for",
        "fun",
        "if",
        "in",
        "interface",
        "is",
        "object",
        "return",
        "super",
        "throw",
        "try",
        "typealias",
        "when",
        "while",
        "by",
        "catch",
        "constructor",
        "finally",
        "init",
        "data",
        "sealed",
        "enum",
        "override",
        "open",
        "abstract",
        "private",
        "public",
        "protected",
        "internal",
        "companion",
        "suspend",
        "inline",
        "lateinit",
        "operator",
    ],
    namespace: &["import", "package"],
    constants: &["true", "false", "null"],
    types: &["Int", "Long", "Short", "Byte", "Double", "Float", "Boolean", "Char", "String", "Unit", "Any", "Nothing"],
    declarations: &["val", "var"],
    pseudo: &["this"],
    def_words: &["fun"],
    class_words: &["class", "interface", "object"],
    decorators: true,
    ..C_LIKE
};

const SWIFT: Lang = Lang {
    names: &["swift"],
    extensions: &["swift"],
    keywords: &[
        "class",
        "struct",
        "enum",
        "protocol",
        "extension",
        "func",
        "if",
        "else",
        "guard",
        "for",
        "in",
        "while",
        "repeat",
        "switch",
        "case",
        "default",
        "return",
        "break",
        "continue",
        "where",
        "init",
        "deinit",
        "try",
        "catch",
        "throw",
        "throws",
        "async",
        "await",
        "public",
        "private",
        "internal",
        "fileprivate",
        "static",
        "override",
        "mutating",
        "inout",
        "some",
        "as",
        "is",
        "defer",
        "do",
    ],
    namespace: &["import"],
    constants: &["true", "false", "nil"],
    types: &[
        "Int",
        "Double",
        "Float",
        "String",
        "Bool",
        "Character",
        "Array",
        "Dictionary",
        "Set",
        "Void",
        "Any",
        "Optional",
    ],
    declarations: &["let", "var"],
    pseudo: &["self", "Self", "super"],
    builtins: &["print"],
    def_words: &["func"],
    class_words: &["class", "struct", "enum", "protocol", "extension"],
    decorators: true,
    ..C_LIKE
};

const JSON: Lang = Lang {
    names: &["json", "jsonc", "json5"],
    extensions: &["json", "jsonc"],
    constants: &["true", "false", "null"],
    quotes: "\"",
    json_keys: true,
    colon_is_punctuation: true,
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    ..BASE
};

const BASH: Lang = Lang {
    names: &["bash", "sh", "shell", "zsh", "ksh", "console", "shell-session"],
    extensions: &["sh", "bash", "zsh"],
    keywords: &[
        "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "until", "case", "esac", "function",
        "return", "select", "time",
    ],
    builtins: &[
        "echo", "cd", "export", "source", "alias", "unset", "read", "printf", "exit", "set", "shift", "test", "eval",
        "exec", "local", "declare", "readonly", "trap", "wait", "kill", "pwd", "type", "true", "false",
    ],
    def_words: &["function"],
    line_comments: &["#"],
    quotes: "\"'",
    dollar_vars: true,
    ..BASE
};

const SQL: Lang = Lang {
    names: &["sql", "mysql", "postgresql", "postgres", "sqlite"],
    extensions: &["sql"],
    keywords: &[
        "select",
        "from",
        "where",
        "and",
        "or",
        "not",
        "insert",
        "into",
        "values",
        "update",
        "set",
        "delete",
        "create",
        "table",
        "drop",
        "alter",
        "add",
        "join",
        "inner",
        "left",
        "right",
        "outer",
        "full",
        "cross",
        "on",
        "as",
        "group",
        "by",
        "order",
        "having",
        "limit",
        "offset",
        "distinct",
        "union",
        "all",
        "exists",
        "in",
        "is",
        "like",
        "between",
        "case",
        "when",
        "then",
        "else",
        "end",
        "primary",
        "key",
        "foreign",
        "references",
        "index",
        "view",
        "default",
        "unique",
        "check",
        "constraint",
        "returning",
        "with",
        "asc",
        "desc",
        "if",
        "begin",
        "commit",
        "rollback",
        "transaction",
    ],
    constants: &["null", "true", "false"],
    types: &[
        "int",
        "integer",
        "varchar",
        "text",
        "boolean",
        "date",
        "timestamp",
        "float",
        "double",
        "decimal",
        "serial",
        "bigint",
        "smallint",
        "char",
        "real",
        "numeric",
        "blob",
        "json",
        "uuid",
    ],
    builtins: &["count", "sum", "avg", "min", "max", "coalesce", "now", "upper", "lower", "length", "round"],
    line_comments: &["--"],
    block_comment: Some(("/*", "*/")),
    quotes: "'\"",
    case_insensitive: true,
    ..BASE
};

const TEXT: Lang = Lang { names: &["text", "plain", "txt", "none", "plaintext"], extensions: &["txt"], ..BASE };

const LANGS: &[&Lang] = &[
    &PYTHON,
    &RUST,
    &JAVASCRIPT,
    &TYPESCRIPT,
    &C,
    &CPP,
    &CSHARP,
    &JAVA,
    &GO,
    &KOTLIN,
    &SWIFT,
    &JSON,
    &BASH,
    &SQL,
    &TEXT,
];

/// Canonical names of the supported languages.
pub fn languages() -> Vec<&'static str> {
    LANGS.iter().map(|l| l.names[0]).collect()
}

fn find_lang(name: &str) -> Option<&'static Lang> {
    let n = name.trim().to_ascii_lowercase();
    LANGS.iter().copied().find(|l| l.names.contains(&n.as_str()))
}

/// The canonical name for a language name or alias (`"py"` → `"python"`).
pub fn language_name(name: &str) -> Option<&'static str> {
    find_lang(name).map(|l| l.names[0])
}

/// The language a file extension (without the dot) belongs to.
pub fn language_for_extension(ext: &str) -> Option<&'static str> {
    let e = ext.trim_start_matches('.').to_ascii_lowercase();
    LANGS.iter().find(|l| l.extensions.contains(&e.as_str())).map(|l| l.names[0])
}

/// Guess the language of a snippet (Pygments' `guess_lexer`); `"text"` when unsure.
pub fn guess_language(code: &str) -> &'static str {
    let t = code.trim_start();
    if t.starts_with("#!") {
        let first = t.lines().next().unwrap_or("");
        if first.contains("python") {
            return "python";
        }
        if first.contains("node") {
            return "javascript";
        }
        if first.contains("sh") {
            return "bash";
        }
    }
    if (t.starts_with('{') || t.starts_with('[')) && looks_like_json(t) {
        return "json";
    }
    let has = |s: &str| code.contains(s);
    let lower = code.to_ascii_lowercase();
    let mut scores = [
        ("python", 0),
        ("rust", 0),
        ("javascript", 0),
        ("typescript", 0),
        ("cpp", 0),
        ("c", 0),
        ("java", 0),
        ("csharp", 0),
        ("go", 0),
        ("sql", 0),
        ("bash", 0),
    ];
    let mut add = |lang: &str, cond: bool, pts: i32| {
        if cond {
            if let Some(s) = scores.iter_mut().find(|s| s.0 == lang) {
                s.1 += pts;
            }
        }
    };
    let line_starts = |p: &str| code.lines().any(|l| l.trim_start().starts_with(p));
    add("python", line_starts("def ") && has("):"), 4);
    add("python", line_starts("import ") || line_starts("from "), 2);
    add("python", has("self") || has("elif ") || has("print("), 2);
    add("python", code.lines().any(|l| l.trim_end().ends_with(':')), 1);
    add("rust", line_starts("fn ") || has(" fn "), 3);
    add("rust", has("let mut ") || has("println!") || has("impl ") || has("::new("), 3);
    add("rust", line_starts("use ") && has("::"), 2);
    add("javascript", has("function ") || has("=> ") || has("console."), 3);
    add("javascript", line_starts("const ") || line_starts("let "), 1);
    add("typescript", has(": string") || has(": number") || line_starts("interface ") || has("export type "), 4);
    add("typescript", has("function ") || has("=> ") || has("console."), 2);
    add("cpp", has("#include") && (has("std::") || has("class ") || has("cout")), 5);
    add("c", has("#include") || has("printf("), 3);
    add("c", has("int main("), 1);
    add("java", has("public class ") || has("System.out") || has("public static void main"), 5);
    add("csharp", has("using System") || has("Console.Write") || has("namespace "), 5);
    add("go", line_starts("package ") || line_starts("func ") || has(":= "), 4);
    add("sql", lower.contains("select ") && lower.contains(" from "), 5);
    add("sql", lower.trim_start().starts_with("create table") || lower.contains("insert into"), 5);
    add("bash", line_starts("echo ") || has("$(") || line_starts("export ") || has(" fi\n") || has("; then"), 3);
    let best = scores.iter().max_by_key(|s| s.1).unwrap();
    if best.1 >= 2 {
        best.0
    } else {
        "text"
    }
}

fn looks_like_json(t: &str) -> bool {
    let body = t.trim_end();
    (body.ends_with('}') || body.ends_with(']')) && !body.contains(';') && !body.contains("function")
}

// ---------------------------------------------------------------------- lexer

struct Lexer<'a> {
    lang: &'a Lang,
    s: Vec<char>,
    i: usize,
    out: Vec<Token>,
    /// What the next identifier is (after `def`, `class`, ...).
    expect: Option<TokenKind>,
}

impl Lexer<'_> {
    fn peek(&self, k: usize) -> Option<char> {
        self.s.get(self.i + k).copied()
    }
    fn at(&self, p: &str) -> bool {
        p.chars().enumerate().all(|(k, c)| self.peek(k) == Some(c))
    }
    fn push(&mut self, kind: TokenKind, from: usize) {
        if from >= self.i {
            return;
        }
        let text: String = self.s[from..self.i].iter().collect();
        match self.out.last_mut() {
            Some(t) if t.kind == kind => t.text.push_str(&text),
            _ => self.out.push(Token { kind, text }),
        }
    }
    fn is_ident_start(&self, c: char) -> bool {
        c.is_alphabetic() || c == '_' || (self.lang.ident_dollar && c == '$')
    }
    fn is_ident(&self, c: char) -> bool {
        c.is_alphanumeric() || c == '_' || (self.lang.ident_dollar && c == '$')
    }
    /// Last non-whitespace character before `pos`.
    fn prev_char(&self, pos: usize) -> Option<char> {
        self.s[..pos].iter().rev().find(|c| !c.is_whitespace()).copied()
    }
    fn line_start_before(&self, pos: usize) -> bool {
        self.s[..pos].iter().rev().take_while(|c| **c != '\n').all(|c| c.is_whitespace())
    }
    fn skip_line(&mut self) {
        while let Some(c) = self.peek(0) {
            if c == '\n' {
                break;
            }
            self.i += 1;
        }
    }

    fn run(mut self) -> Vec<Token> {
        let lang = self.lang;
        while let Some(c) = self.peek(0) {
            let start = self.i;
            if c.is_whitespace() {
                while self.peek(0).is_some_and(char::is_whitespace) {
                    self.i += 1;
                }
                self.push(TokenKind::Text, start);
                continue;
            }
            // Comments.
            if let Some(p) = lang.line_comments.iter().find(|p| self.at(p)) {
                let ok = *p != "#" || !lang.dollar_vars || start == 0 || self.s[start - 1].is_whitespace();
                if ok {
                    self.skip_line();
                    self.push(TokenKind::Comment, start);
                    self.expect = None;
                    continue;
                }
            }
            if let Some((open, close)) = lang.block_comment {
                if self.at(open) {
                    self.i += open.chars().count();
                    while self.peek(0).is_some() && !self.at(close) {
                        self.i += 1;
                    }
                    self.i = (self.i + close.chars().count()).min(self.s.len());
                    self.push(TokenKind::Comment, start);
                    continue;
                }
            }
            if lang.hash_preproc && c == '#' && self.line_start_before(start) {
                loop {
                    self.skip_line();
                    if self.i > 0 && self.s[self.i - 1] == '\\' && self.peek(0) == Some('\n') {
                        self.i += 1;
                        continue;
                    }
                    break;
                }
                self.push(TokenKind::Preproc, start);
                continue;
            }
            if lang.rust && c == '#' && matches!(self.peek(1), Some('[') | Some('!')) {
                let mut depth = 0;
                while let Some(ch) = self.peek(0) {
                    self.i += 1;
                    match ch {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        '\n' if depth == 0 => break,
                        _ => {}
                    }
                }
                self.push(TokenKind::Preproc, start);
                continue;
            }
            if lang.decorators && c == '@' && self.peek(1).is_some_and(|n| self.is_ident_start(n)) {
                self.i += 1;
                while self.peek(0).is_some_and(|n| self.is_ident(n) || n == '.') {
                    self.i += 1;
                }
                self.push(TokenKind::Decorator, start);
                continue;
            }
            if lang.dollar_vars && c == '$' {
                self.dollar_var();
                continue;
            }
            // Strings.
            if lang.rust && c == '\'' {
                // A lifetime ('a) unless it closes like a char literal ('a', '\n').
                let is_char = self.peek(1) == Some('\\') || self.peek(2) == Some('\'');
                if !is_char && self.peek(1).is_some_and(|n| self.is_ident_start(n)) {
                    self.i += 1;
                    while self.peek(0).is_some_and(|n| self.is_ident(n)) {
                        self.i += 1;
                    }
                    self.push(TokenKind::Attribute, start);
                    continue;
                }
            }
            if lang.quotes.contains(c) {
                self.string(start, false, false);
                continue;
            }
            // Numbers.
            if c.is_ascii_digit() || (c == '.' && self.peek(1).is_some_and(|n| n.is_ascii_digit())) {
                self.number();
                self.push(TokenKind::Number, start);
                self.expect = None;
                continue;
            }
            if self.is_ident_start(c) {
                while self.peek(0).is_some_and(|n| self.is_ident(n)) {
                    self.i += 1;
                }
                let word: String = self.s[start..self.i].iter().collect();
                // String prefixes: Python r"..", f'..', b"..; Rust b"..", r#".."#.
                if let Some(q) = self.peek(0).filter(|q| lang.quotes.contains(*q)) {
                    let w = word.to_ascii_lowercase();
                    if lang.python_strings && w.len() <= 2 && w.chars().all(|ch| "rbuf".contains(ch)) {
                        self.string(start, w.contains('r'), w.contains('f'));
                        continue;
                    }
                    if lang.rust && (w == "b" || w == "r" || w == "br") && q == '"' {
                        self.string(start, w.contains('r'), false);
                        continue;
                    }
                }
                if lang.rust && (word == "r" || word == "br") && self.peek(0) == Some('#') {
                    let hashes = self.s[self.i..].iter().take_while(|h| **h == '#').count();
                    if self.peek(hashes) == Some('"') {
                        self.i += hashes + 1;
                        let close: String = std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect();
                        while self.peek(0).is_some() && !self.at(&close) {
                            self.i += 1;
                        }
                        self.i = (self.i + close.len()).min(self.s.len());
                        self.push(TokenKind::String, start);
                        continue;
                    }
                }
                if lang.rust
                    && self.peek(0) == Some('!')
                    && self.peek(1) != Some('=')
                    && !lang.keywords.contains(&word.as_str())
                {
                    self.i += 1;
                    self.push(TokenKind::Macro, start);
                    self.expect = None;
                    continue;
                }
                let kind = self.classify(&word, start);
                self.push(kind, start);
                continue;
            }
            // Operators and punctuation.
            self.i += 1;
            let kind = match c {
                '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' => TokenKind::Punctuation,
                ':' if lang.colon_is_punctuation => TokenKind::Punctuation,
                '.' if !lang.dot_is_operator => TokenKind::Punctuation,
                '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!' | '&' | '|' | '^' | '~' | '?' | ':' | '.' | '@' => {
                    TokenKind::Operator
                }
                _ => TokenKind::Text,
            };
            self.expect = None;
            self.push(kind, start);
        }
        self.out
    }

    fn classify(&mut self, word: &str, start: usize) -> TokenKind {
        let lang = self.lang;
        let lower;
        let w = if lang.case_insensitive {
            lower = word.to_ascii_lowercase();
            lower.as_str()
        } else {
            word
        };
        if let Some(k) = self.expect.take() {
            if !lang.keywords.contains(&w) && !lang.declarations.contains(&w) {
                return k;
            }
        }
        let after_dot = self.prev_char(start) == Some('.');
        let in_list = |l: &[&str]| l.contains(&w);
        if in_list(lang.def_words) {
            self.expect = Some(TokenKind::Function);
        } else if in_list(lang.class_words) {
            self.expect = Some(TokenKind::Class);
        }
        if in_list(lang.declarations) {
            return TokenKind::KeywordDeclaration;
        }
        if in_list(lang.namespace) {
            return TokenKind::KeywordNamespace;
        }
        if in_list(lang.keywords) {
            return TokenKind::Keyword;
        }
        if in_list(lang.operator_words) {
            return TokenKind::OperatorWord;
        }
        if after_dot {
            return TokenKind::Text;
        }
        if in_list(lang.constants) {
            return TokenKind::KeywordConstant;
        }
        if in_list(lang.types) {
            return TokenKind::KeywordType;
        }
        if in_list(lang.pseudo) {
            return TokenKind::BuiltinPseudo;
        }
        if in_list(lang.builtins) {
            return TokenKind::Builtin;
        }
        TokenKind::Text
    }

    fn number(&mut self) {
        let hex = self.peek(0) == Some('0') && matches!(self.peek(1), Some('x' | 'X' | 'b' | 'B' | 'o' | 'O'));
        if hex {
            self.i += 2;
            while self.peek(0).is_some_and(|c| c.is_ascii_hexdigit() || c == '_') {
                self.i += 1;
            }
        } else {
            while self.peek(0).is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.i += 1;
            }
            if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
                while self.peek(0).is_some_and(|c| c.is_ascii_digit() || c == '_') {
                    self.i += 1;
                }
            }
            if matches!(self.peek(0), Some('e' | 'E')) {
                let k = if matches!(self.peek(1), Some('+' | '-')) { 2 } else { 1 };
                if self.peek(k).is_some_and(|c| c.is_ascii_digit()) {
                    self.i += k;
                    while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                        self.i += 1;
                    }
                }
            }
        }
        // Suffixes: 10u32, 1.5f, 3j, 10n, 7L.
        while self.peek(0).is_some_and(|c| c.is_ascii_alphanumeric()) {
            self.i += 1;
        }
    }

    fn dollar_var(&mut self) {
        let start = self.i;
        self.i += 1;
        match self.peek(0) {
            Some('{') => {
                while let Some(c) = self.peek(0) {
                    self.i += 1;
                    if c == '}' || c == '\n' {
                        break;
                    }
                }
            }
            Some('(') => {
                // Command substitution: color the `$(` and lex the rest normally.
                self.i += 1;
                self.push(TokenKind::Operator, start);
                return;
            }
            Some(c) if c.is_alphanumeric() || c == '_' => {
                while self.peek(0).is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    self.i += 1;
                }
            }
            Some(c) if "@#?$!*-".contains(c) => self.i += 1,
            _ => {}
        }
        self.push(TokenKind::Variable, start);
    }

    /// A string starting at `start` (prefix letters included); the cursor is on the quote.
    fn string(&mut self, start: usize, raw: bool, fstring: bool) {
        let lang = self.lang;
        let q = self.peek(0).unwrap();
        let triple = lang.python_strings && self.peek(1) == Some(q) && self.peek(2) == Some(q);
        let doc = triple && self.line_start_before(start);
        let body = if doc { TokenKind::StringDoc } else { TokenKind::String };
        let template = lang.template_strings && q == '`';
        let multiline = triple || template || (q == '`') || (lang.rust && q == '"');
        self.i += if triple { 3 } else { 1 };
        let mut seg = start;
        while let Some(c) = self.peek(0) {
            if triple && c == q && self.peek(1) == Some(q) && self.peek(2) == Some(q) {
                self.i += 3;
                break;
            }
            if !triple && c == q {
                self.i += 1;
                break;
            }
            if c == '\n' && !multiline {
                break;
            }
            if c == '\\' && !raw {
                self.push(body, seg);
                let e = self.i;
                self.i += 1;
                match self.peek(0) {
                    Some('x') => self.i += 3,
                    Some('u') if self.peek(1) == Some('{') => {
                        while self.peek(0).is_some_and(|c| c != '}' && c != q) {
                            self.i += 1;
                        }
                        self.i += 1;
                    }
                    Some('u') => self.i += 5,
                    Some(_) => self.i += 1,
                    None => {}
                }
                self.i = self.i.min(self.s.len());
                self.push(TokenKind::StringEscape, e);
                seg = self.i;
                continue;
            }
            let interp = (fstring && c == '{' && self.peek(1) != Some('{'))
                || (template && c == '$' && self.peek(1) == Some('{'))
                || (lang.dollar_vars && q == '"' && c == '$');
            if fstring && c == '{' && self.peek(1) == Some('{') {
                self.i += 2;
                continue;
            }
            if interp {
                self.push(body, seg);
                if lang.dollar_vars {
                    self.dollar_var();
                } else {
                    let e = self.i;
                    let mut depth = 0;
                    while let Some(ch) = self.peek(0) {
                        self.i += 1;
                        match ch {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            '\n' if !multiline => break,
                            _ => {}
                        }
                    }
                    self.push(TokenKind::StringInterpol, e);
                }
                seg = self.i;
                continue;
            }
            self.i += 1;
        }
        // A JSON object key is a string followed by ':'.
        let key = lang.json_keys && self.s[self.i..].iter().find(|c| !c.is_whitespace()) == Some(&':');
        self.push(if key { TokenKind::Tag } else { body }, seg);
        self.expect = None;
    }
}

/// Split `code` into tokens. The token texts concatenate back to `code`.
/// Fails for an unknown language; [`languages`] lists the known ones.
pub fn tokenize(code: &str, language: &str) -> anyhow::Result<Vec<Token>> {
    let Some(lang) = find_lang(language) else {
        anyhow::bail!("unknown language '{language}' (known: {})", languages().join(", "))
    };
    Ok(Lexer { lang, s: code.chars().collect(), i: 0, out: Vec::new(), expect: None }.run())
}

// ---------------------------------------------------------------------- styles

/// A color style: background, default text, line numbers, and a color per token class.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    /// Pygments style name, e.g. `"vim"`.
    pub name: &'static str,
    /// Background color as `#RRGGBB`.
    pub background: &'static str,
    /// Default text color as `#RRGGBB`.
    pub text: &'static str,
    /// Line number color as `#RRGGBB`.
    pub line_number: &'static str,
    /// Colors for `TokenKind::Keyword ..= TokenKind::Variable`; `None` uses `text`.
    colors: [Option<&'static str>; 24],
}

impl Style {
    /// Hex color of a token class.
    pub fn color(&self, kind: TokenKind) -> &'static str {
        match kind {
            TokenKind::Text => self.text,
            k => self.colors[k as usize - 1].unwrap_or(self.text),
        }
    }
}

macro_rules! styles {
    ($( ($name:literal, $bg:literal, $fg:literal, $ln:literal, [$($c:tt),*]) ),* $(,)?) => {
        &[$( Style { name: $name, background: $bg, text: $fg, line_number: $ln, colors: [$(styles!(@c $c)),*] } ),*]
    };
    (@c None) => { None };
    (@c $c:literal) => { Some($c) };
}

// Generated from Pygments 2.21 (`style_for_token` for each class).
#[rustfmt::skip]
const STYLES: &[Style] = styles![
    ("vim", "#000000", "#cccccc", "#cccccc", ["#cdcd00", "#cd00cd", "#cdcd00", "#00cd00", "#00cd00", "#cd00cd", "#cd00cd", "#cccccc", "#00cdcd", "#cccccc", "#cccccc", "#cccccc", "#cd0000", "#cd0000", "#cd0000", "#cd0000", "#cd00cd", "#000080", "#000080", "#3399cc", "#cdcd00", "#cccccc", "#cccccc", "#00cdcd"]),
    ("monokai", "#272822", "#f8f8f2", "#f8f8f2", ["#66d9ef", "#ff4689", "#66d9ef", "#66d9ef", "#66d9ef", "#a6e22e", "#a6e22e", "#a6e22e", "#a6e22e", "#a6e22e", "#a6e22e", "#a6e22e", "#e6db74", "#ae81ff", "#e6db74", "#e6db74", "#ae81ff", "#959077", "#959077", "#ff4689", "#ff4689", "#f8f8f2", "#ff4689", "#f8f8f2"]),
    ("dracula", "#282a36", "#f8f8f2", "#f1fa8c", ["#ff79c6", "#ff79c6", "#ff79c6", "#8be9fd", "#8be9fd", "#8be9fd", "#f8f8f2", "#50fa7b", "#50fa7b", "#f8f8f2", "#50fa7b", "#50fa7b", "#bd93f9", "#bd93f9", "#bd93f9", "#bd93f9", "#ffb86c", "#6272a4", "#ff79c6", "#ff79c6", "#ff79c6", "#f8f8f2", "#ff79c6", "#8be9fd"]),
    ("github-dark", "#0d1117", "#e6edf3", "#6e7681", ["#ff7b72", "#ff7b72", "#79c0ff", "#ff7b72", "#ff7b72", "#e6edf3", "#e6edf3", "#d2a8ff", "#f0883e", "#d2a8ff", "#d2a8ff", "#e6edf3", "#a5d6ff", "#79c0ff", "#a5d6ff", "#a5d6ff", "#a5d6ff", "#8b949e", "#8b949e", "#ff7b72", "#ff7b72", "#e6edf3", "#7ee787", "#79c0ff"]),
    ("one-dark", "#282C34", "#ABB2BF", "#ABB2BF", ["#C678DD", "#C678DD", "#E5C07B", "#E5C07B", "#C678DD", "#E5C07B", "#E5C07B", "#61AFEF", "#E5C07B", "#61AFEF", "#56B6C2", "#E06C75", "#98C379", "#98C379", "#98C379", "#98C379", "#D19A66", "#7F848E", "#7F848E", "#56B6C2", "#56B6C2", "#ABB2BF", "#E06C75", "#E06C75"]),
    ("nord", "#2E3440", "#d8dee9", "#D8DEE9", ["#81a1c1", "#81a1c1", "#81a1c1", "#81a1c1", "#81a1c1", "#81a1c1", "#81a1c1", "#88c0d0", "#8fbcbb", "#d08770", "#88c0d0", "#8fbcbb", "#a3be8c", "#ebcb8b", "#a3be8c", "#616e87", "#b48ead", "#616e87", "#5e81ac", "#81a1c1", "#81a1c1", "#eceff4", "#81a1c1", "#d8dee9"]),
    ("gruvbox-dark", "#282828", "#dddddd", "#dddddd", ["#fb4934", "#fb4934", "#fb4934", "#fb4934", "#fb4934", "#fe8019", "#fe8019", "#8ec07c", "#8ec07c", "#fb4934", "#8ec07c", "#fabd2f", "#b8bb26", "#fe8019", "#b8bb26", "#b8bb26", "#d3869b", "#928374", "#928374", "#dddddd", "#fb4934", "#dddddd", "#8ec07c", "#83a598"]),
    ("gruvbox-light", "#fbf1c7", "#000000", "#000000", ["#9d0006", "#9d0006", "#9d0006", "#9d0006", "#9d0006", "#af3a03", "#af3a03", "#427b58", "#427b58", "#9d0006", "#427b58", "#b57614", "#79740e", "#af3a03", "#79740e", "#79740e", "#8f3f71", "#928374", "#928374", None, "#9d0006", None, "#427b58", "#076678"]),
    ("solarized-dark", "#002b36", "#839496", "#586e75", ["#859900", "#cb4b16", "#2aa198", "#b58900", "#2aa198", "#268bd2", "#268bd2", "#268bd2", "#268bd2", "#268bd2", "#268bd2", "#839496", "#2aa198", "#2aa198", "#2aa198", "#586e75", "#2aa198", "#586e75", "#d33682", "#586e75", "#859900", "#839496", "#268bd2", "#268bd2"]),
    ("solarized-light", "#fdf6e3", "#657b83", "#93a1a1", ["#859900", "#cb4b16", "#2aa198", "#b58900", "#2aa198", "#268bd2", "#268bd2", "#268bd2", "#268bd2", "#268bd2", "#268bd2", "#657b83", "#2aa198", "#2aa198", "#2aa198", "#93a1a1", "#2aa198", "#93a1a1", "#d33682", "#93a1a1", "#859900", "#657b83", "#268bd2", "#268bd2"]),
    ("default", "#f8f8f8", "#000000", "#000000", ["#008000", "#008000", "#008000", "#B00040", "#008000", "#008000", "#008000", "#0000FF", "#0000FF", "#AA22FF", "#0000FF", "#687822", "#BA2121", "#AA5D1F", "#A45A77", "#BA2121", "#666666", "#3D7B7B", "#9C6500", "#666666", "#AA22FF", None, "#008000", "#19177C"]),
    ("friendly", "#f0f0f0", "#000000", "#666666", ["#007020", "#007020", "#007020", "#902000", "#007020", "#007020", "#007020", "#06287e", "#0e84b5", "#555555", "#06287e", "#4070a0", "#4070a0", "#4070a0", "#70a0d0", "#4070a0", "#40a070", "#60a0b0", "#007020", "#666666", "#007020", None, "#062873", "#bb60d5"]),
    ("native", "#202020", "#d0d0d0", "#aaaaaa", ["#6ebf26", "#6ebf26", "#6ebf26", "#6ebf26", "#6ebf26", "#2fbccd", "#2fbccd", "#71adff", "#71adff", "#ffa500", "#71adff", "#bbbbbb", "#ed9d13", "#ed9d13", "#ed9d13", "#ed9d13", "#51b2fd", "#ababab", "#ff3a3a", "#d0d0d0", "#6ebf26", "#d0d0d0", "#6ebf26", "#40ffff"]),
    ("material", "#263238", "#EEFFFF", "#37474F", ["#BB80B3", "#89DDFF", "#89DDFF", "#BB80B3", "#BB80B3", "#82AAFF", "#89DDFF", "#82AAFF", "#FFCB6B", "#82AAFF", "#82AAFF", "#BB80B3", "#C3E88D", "#EEFFFF", "#89DDFF", "#546E7A", "#F78C6C", "#546E7A", "#546E7A", "#89DDFF", "#89DDFF", "#89DDFF", "#FF5370", "#89DDFF"]),
    ("zenburn", "#3f3f3f", "#dcdccc", "#5d6262", ["#efdcbc", "#f0dfaf", "#dca3a3", "#dfdfbf", "#f0dfaf", "#efef8f", "#dcdccc", "#efef8f", "#efef8f", "#dcdccc", "#efef8f", "#efef8f", "#cc9393", "#cc9393", "#dca3a3", "#7f9f7f", "#8cd0d3", "#7f9f7f", "#dfaf8f", "#f0efd0", "#f0efd0", "#f0efd0", "#e89393", "#dcdccc"]),
    ("xcode", "#ffffff", "#000000", "#000000", ["#A90D91", "#A90D91", "#A90D91", "#A90D91", "#A90D91", "#A90D91", "#5B269A", "#000000", "#3F6E75", "#000000", "#000000", "#836C28", "#C41A16", "#C41A16", "#C41A16", "#C41A16", "#1C01CE", "#177500", "#633820", "#000000", "#000000", None, "#000000", "#000000"]),
    ("vs", "#ffffff", "#000000", "#000000", ["#0000ff", "#0000ff", "#0000ff", "#2b91af", "#0000ff", None, None, None, "#2b91af", None, None, None, "#a31515", "#a31515", "#a31515", "#a31515", None, "#008000", "#0000ff", None, "#0000ff", None, None, None]),
    ("fruity", "#111111", "#ffffff", "#ffffff", ["#fb660a", "#fb660a", "#fb660a", "#cdcaa9", "#fb660a", "#ffffff", "#ffffff", "#ff0086", "#ffffff", "#ffffff", "#ff0086", "#ff0086", "#0086d2", "#0086d2", "#0086d2", "#0086d2", "#0086f7", "#008800", "#ff0007", "#ffffff", "#ffffff", "#ffffff", "#fb660a", "#fb660a"]),
    ("rrt", "#000000", "#dddddd", "#dddddd", ["#ff0000", "#ff0000", "#ff0000", "#ee82ee", "#ff0000", "#dddddd", "#dddddd", "#ffff00", "#dddddd", "#dddddd", "#ffff00", "#dddddd", "#87ceeb", "#87ceeb", "#87ceeb", "#87ceeb", "#ff00ff", "#00ff00", "#e5e5e5", "#dddddd", "#ff0000", "#dddddd", "#7fffd4", "#eedd82"]),
    ("emacs", "#f8f8f8", "#000000", "#000000", ["#AA22FF", "#AA22FF", "#AA22FF", "#00BB00", "#AA22FF", "#AA22FF", "#AA22FF", "#00A000", "#0000FF", "#AA22FF", "#00A000", "#BB4444", "#BB4444", "#BB6622", "#BB6688", "#BB4444", "#666666", "#008800", "#008800", "#666666", "#AA22FF", None, "#008000", "#B8860B"]),
];

/// Names of the available styles (Manim's `Code.get_styles_list()`).
pub fn styles() -> Vec<&'static str> {
    STYLES.iter().map(|s| s.name).collect()
}

/// A style by name (case and `_`/`-` don't matter: `"github_dark"` works).
pub fn style(name: &str) -> anyhow::Result<&'static Style> {
    let key = name.trim().to_ascii_lowercase().replace('_', "-");
    let key = match key.as_str() {
        "onedark" => "one-dark",
        "githubdark" | "github" => "github-dark",
        "solarized" => "solarized-dark",
        "gruvbox" => "gruvbox-dark",
        other => other,
    };
    STYLES
        .iter()
        .find(|s| s.name == key)
        .ok_or_else(|| anyhow::anyhow!("unknown code style '{name}' (known: {})", styles().join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use TokenKind::*;

    fn kinds(code: &str, lang: &str) -> Vec<(TokenKind, std::string::String)> {
        let toks = tokenize(code, lang).unwrap();
        assert_eq!(
            toks.iter().map(|t| t.text.as_str()).collect::<std::string::String>(),
            code,
            "tokens must cover the source"
        );
        toks.into_iter().filter(|t| !t.text.trim().is_empty()).map(|t| (t.kind, t.text)).collect()
    }

    fn kind_of(code: &str, lang: &str, text: &str) -> TokenKind {
        kinds(code, lang).into_iter().find(|t| t.1 == text).unwrap_or_else(|| panic!("no token {text:?}")).0
    }

    #[test]
    fn python_like_pygments() {
        let src = "from manim import Scene\n\n@dataclass\nclass Sq(Scene):\n    def construct(self):\n        \"\"\"Doc.\"\"\"\n        s = f\"x{n}\\n\"  # hi\n        return not True and 0x1F + 1.5e3\n";
        assert_eq!(kind_of(src, "python", "from"), KeywordNamespace);
        assert_eq!(kind_of(src, "python", "@dataclass"), Decorator);
        assert_eq!(kind_of(src, "python", "class"), Keyword);
        assert_eq!(kind_of(src, "python", "Sq"), Class);
        assert_eq!(kind_of(src, "python", "construct"), Function);
        assert_eq!(kind_of(src, "python", "self"), BuiltinPseudo);
        assert_eq!(kind_of(src, "python", "\"\"\"Doc.\"\"\""), StringDoc);
        assert_eq!(kind_of(src, "python", "{n}"), StringInterpol);
        assert_eq!(kind_of(src, "python", "\\n"), StringEscape);
        assert_eq!(kind_of(src, "python", "f\"x"), String);
        assert_eq!(kind_of(src, "python", "# hi"), Comment);
        assert_eq!(kind_of(src, "python", "not"), OperatorWord);
        assert_eq!(kind_of(src, "python", "True"), KeywordConstant);
        assert_eq!(kind_of(src, "python", "0x1F"), Number);
        assert_eq!(kind_of(src, "python", "1.5e3"), Number);
        assert_eq!(kind_of(src, "python", "="), Operator);
        assert_eq!(kind_of("a.b", "python", "."), Operator);
        assert_eq!(kind_of("x.print", "python", "print"), Text, "attributes are not builtins");
    }

    #[test]
    fn rust_like_pygments() {
        let src = "#[derive(Debug)]\nfn main<'a>() {\n    let x: u32 = 42; // c\n    println!(\"{x}\\n\");\n    Some(r#\"raw\"#)\n}\n";
        assert_eq!(kind_of(src, "rust", "#[derive(Debug)]"), Preproc);
        assert_eq!(kind_of(src, "rust", "main"), Function);
        assert_eq!(kind_of(src, "rust", "'a"), Attribute);
        assert_eq!(kind_of(src, "rust", "let"), KeywordDeclaration);
        assert_eq!(kind_of(src, "rust", "u32"), KeywordType);
        assert_eq!(kind_of(src, "rust", "42"), Number);
        assert_eq!(kind_of(src, "rust", "println!"), Macro);
        assert_eq!(kind_of(src, "rust", "Some"), Builtin);
        assert_eq!(kind_of(src, "rust", "r#\"raw\"#"), String);
        assert_eq!(kind_of("let c = 'x';", "rust", "'x'"), String);
        assert_eq!(kind_of("for i in 0..10 {}", "rust", "0"), Number);
    }

    #[test]
    fn other_languages() {
        assert_eq!(kind_of("#include <stdio.h>\nint main() { return 0; }", "c", "#include <stdio.h>"), Preproc);
        assert_eq!(kind_of("int x;", "c", "int"), KeywordType);
        assert_eq!(kind_of("const s = `a${b}c`;", "js", "${b}"), StringInterpol);
        assert_eq!(kind_of("function f() {}", "javascript", "f"), Function);
        assert_eq!(kind_of("{\"key\": \"v\"}", "json", "\"key\""), Tag);
        assert_eq!(kind_of("{\"key\": \"v\"}", "json", "\"v\""), String);
        assert_eq!(kind_of("echo \"$HOME\" # c", "bash", "$HOME"), Variable);
        assert_eq!(kind_of("echo \"$HOME\" # c", "bash", "# c"), Comment);
        assert_eq!(kind_of("SELECT a FROM t", "sql", "SELECT"), Keyword);
        assert_eq!(kind_of("func main() {}", "go", "main"), Function);
        assert!(tokenize("x", "cobol").is_err());
        assert_eq!(kinds("anything goes", "text").len(), 1);
    }

    #[test]
    fn guesses() {
        assert_eq!(guess_language("def f(x):\n    return x\n"), "python");
        assert_eq!(guess_language("fn main() {\n    println!(\"hi\");\n}"), "rust");
        assert_eq!(guess_language("{\"a\": 1}"), "json");
        assert_eq!(guess_language("SELECT * FROM users;"), "sql");
        assert_eq!(guess_language("#include <stdio.h>\nint main() {}"), "c");
        assert_eq!(guess_language("hello world"), "text");
        assert_eq!(language_for_extension("tsx"), Some("typescript"));
        assert_eq!(language_name("py"), Some("python"));
    }

    #[test]
    fn style_lookup() {
        assert_eq!(style("vim").unwrap().color(Keyword), "#cdcd00");
        assert_eq!(style("Monokai").unwrap().background, "#272822");
        assert_eq!(style("github_dark").unwrap().name, "github-dark");
        assert_eq!(style("vs").unwrap().color(Builtin), "#000000", "missing colors fall back to text");
        assert!(style("nope").is_err());
        assert!(styles().len() >= 20);
    }
}
