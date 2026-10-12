; Zed gives a node the capture of the last pattern that matches it, so general
; patterns come first and specific ones after them.

(identifier) @variable

(type_identifier) @type

((type_identifier) @type.builtin
  (#any-of? @type.builtin
    "Int" "F64" "F32" "Bool" "Str" "Unit" "List" "Opt" "Res" "Map" "Set" "Heap"
    "StrBuf" "Time" "Duration" "Bits" "HashMap" "HashSet" "BigInt" "Dec" "Regex"
    "Rng" "Complex" "Ratio" "Locale" "Zone" "File" "View" "FlatMap" "MdSpan"
    "DevBuf" "Atomic" "Chan" "Secret" "Pii" "Untrusted" "Guess" "Ptr" "Array"
    "Mmio" "Self" "I8" "I16" "I32" "U8" "U16" "U32" "U64"))

; Constructors: a capitalized name with fields or an argument, after `|`, `=`
; or `raise`, and the built-in ones.
(_
  (type_identifier) @constructor
  .
  [(brace_group) (paren_group)])

(_
  ["|" "raise"]
  .
  (type_identifier) @constructor)

; Words that are keywords only at the start of a signature line or definition;
; a call such as `drop(x)` stays a call.
(source_file
  (identifier) @keyword
  (#any-of? @keyword
    "profile" "derive" "pre" "post" "ex" "new" "dec" "cost" "queue" "ep"
    "interrupt" "from" "as" "drop" "migrate"))

; Calls and members.
(_
  (identifier) @function
  .
  (paren_group))

(member
  name: (identifier) @property)

(_
  (member
    name: (identifier) @function.method)
  .
  (paren_group))

(member
  name: (type_identifier) @type)

(_
  (member
    name: (type_identifier) @constructor)
  .
  [(brace_group) (paren_group)])

((identifier) @constructor
  (#any-of? @constructor "some" "none" "ok" "err"))

; Definitions.
(function_definition
  name: (identifier) @function)

(effect_definition
  name: (identifier) @function)

(rule_definition
  name: (identifier) @function)

(test_definition
  name: (identifier) @function)

(service_definition
  name: (identifier) @function)

(type_definition
  name: (type_identifier) @type)

(trait_definition
  name: (type_identifier) @type)

(store_definition
  name: (type_identifier) @type)

(scoped_type_identifier
  package: (identifier) @namespace)

(parameters
  (identifier) @variable.parameter
  .
  ":")

(brace_group
  (identifier) @property
  .
  ":")

(type_parameters
  (identifier) @type)

; Effects in a signature, `! log, fail[E]`.
(_
  "!"
  .
  (identifier) @attribute)

; `@grid(n, 256)` on a kernel fn.
(_
  "@"
  .
  (identifier) @attribute)

; Words that are keywords in their constructs.
((identifier) @keyword
  (#any-of? @keyword
    "is" "pub" "use" "extern" "kernel" "res" "static" "unsafe" "asm" "mut"
    "resume" "yield" "let" "guard" "elif"))

(placeholder) @variable.special

; Literals.
(string_content) @string

(string
  ["\"" "'"] @string)

(escaped_string) @string

(escape_sequence) @string.escape

(integer) @number

(float) @number

(boolean) @boolean

(hole) @string.special

(hash) @string.special.symbol

(comment) @comment

; Keywords and punctuation.
[
  "fn"
  "type"
  "test"
  "trait"
  "impl"
  "effect"
  "store"
  "svc"
  "rule"
  "var"
] @keyword

[
  "if"
  "then"
  "else"
  "match"
  "catch"
  "handle"
  "do"
  "raise"
  "return"
  "for"
  "in"
  "while"
  "par"
  "with"
  "where"
] @keyword

[
  "and"
  "or"
  "not"
] @keyword.operator

[
  "=="
  "!="
  "<="
  ">="
  "<"
  ">"
  "+"
  "-"
  "*"
  "/"
  "%"
  "**"
  "="
  ":="
  "->"
  "=>"
  ".."
  "..."
  "!"
  "?"
  "&"
  "@"
] @operator

[
  ","
  ":"
  ";"
  "."
] @punctuation.delimiter

"|" @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

(interpolation
  "{" @punctuation.special
  "}" @punctuation.special)
