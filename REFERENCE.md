# &lt;name, formerly larksson&gt; — Language Reference

This document describes the syntax and semantics of the language. It is intended as a spec to be looked up, not a tutorial.

---

## 1. Values

The runtime has exactly two value forms:

- **Integer** — a signed 32-bit number.
- **Map** — an ordered map from values to values, keyed by structural equality and sorted by structural order.

Every other syntactic form in the language is sugar that desugars to one of these two at parse time.

| Source              | Desugars to                                              |
|---------------------|----------------------------------------------------------|
| `42`, `-7`          | Integer                                                  |
| `'a'`               | Integer `97` (the codepoint)                             |
| `"hi"`              | Map `{0: 104, 1: 105}`                                   |
| `hi`                | Map `{0: 104, 1: 105}` — bare identifiers are strings    |
| `[1, 2, 3]`         | Map `{0: 1, 1: 2, 2: 3}`                                 |
| `.a.b.c.`           | Map `{0: "a", 1: "b", 2: "c"}` — components are values   |
| `{1: 2, 3: 4}`      | Map `{1: 2, 3: 4}` — explicit map literal                |
| `(<value>)`         | A deref expression — see §6                              |

Strings are not a distinct runtime type. `"hi"`, `hi`, and `[104, 105]` produce identical values.

### 1.1 Escape sequences

Inside `'...'` and `"..."` literals, the following escapes are recognized: `\n`, `\t`, `\r`, `\0`, `\\`, `\'`, `\"`. Any other backslashed character is a parse error.

### 1.2 Identifiers

Bare identifiers must start with an ASCII letter or digit and contain only ASCII alphanumerics. Identifiers always desugar to the string of their characters.

---

## 2. Paths

A **path** is any map whose entries at integer keys `0, 1, 2, …` form a sequence. The path's components are those entries, in order, stopping at the first missing index. Non-integer keys and keys past a gap are silently ignored.

```
{0: a, 1: b, 2: c}      => path [a, b, c]
{0: a, 1: b, 99: x}     => path [a, b]
{1: x, 2: y}            => empty path (no key 0)
{}                      => empty path
```

The dot-path sugar (`.a.b.c.`) and the array sugar (`[a, b, c]`) are both designed to produce path-shaped maps.

Paths name locations in root. **Walking** a path means: starting at root, follow each component in order, treating the current value as a map and the component as a key. Walking through a non-map value, or following a missing key, is a runtime error.

---

## 3. Statements

A **statement** is a three-element indexed map of the form `{0: lhs, 1: rhs, 2: depth}`.

| Source     | Desugars to              | Meaning              |
|------------|--------------------------|----------------------|
| `a = b`    | `{0: a, 1: b, 2: 0}`     | Eager assignment     |
| `a <- b`   | `{0: a, 1: b, 2: 1}`     | Deferred assignment  |

### 3.1 Execution

When a statement runs:

1. The LHS is evaluated, then interpreted as a path.
2. The RHS is evaluated to a value `v`.
3. If `depth > 0`, the value `v` is interpreted as a path, walked through root, and the result replaces `v`. This is repeated `depth` times.
4. The final `v` is assigned at the LHS path.

Eager assignment (`=`) is depth 0: the RHS is stored as-is. Deferred assignment (`<-`) is depth 1: the RHS is treated as a path-to-the-real-source, and one extra walk is performed at execution time.

Depths greater than 1 are not produced by source syntax but are accepted by the runtime. A statement constructed manually with depth 2 will perform two consecutive path-walks on the RHS before assigning.

Statements with no depth slot (`{0: lhs, 1: rhs}`) are accepted and treated as eager.

### 3.2 Assignment semantics

Assignment auto-creates intermediate maps along the LHS path. If a non-map value is encountered while walking, it is destructively replaced with an empty map and walking continues. This means writing to `.var.foo.bar.` when `.var.foo.` is currently an integer will overwrite `.var.foo.` with `{bar: ...}`.

The empty path (LHS that evaluates to `{}`) is **not** a permitted target — see §5.

---

## 4. Programs

A **program** is a map whose entries at consecutive integer keys are statements. Programs run by executing their statements in order of key.

There are two ways to write a program:

**File-level**, prefixed with `lines!`:

```
lines!
.var.x. = 5
.var.y. = 10
```

**As a value**, by constructing the statement triples manually:

```
.var.prog. = [[.var.x., 5, 0], [.var.y., 10, 0]]
```

The two forms produce structurally identical maps. The `lines!` marker exists to distinguish a file that should be executed as a program from a file that is pure data.

A file that does not start with `lines!` is a single value. The current runtime still attempts to execute the top-level value as a program; if it is not program-shaped, the runtime panics.

---

## 5. Reserved namespaces

The first component of every path written to is checked at runtime. It must be one of:

- `ops` — operations (see §7).
- `var` — user data.

Any other first component is a runtime panic.

Within `.ops.<name>.`, the second component (`<name>`) must be the name of a registered op. Writing to `.ops.foo.` where `foo` is not registered panics.

Within `.var.`, anything is permitted.

The empty path is not a valid write target. Writes that resolve to an empty path panic.

---

## 6. Derefs

The form `(<value>)` is a **deref expression**. At evaluation time:

1. The inner value is evaluated.
2. The result is interpreted as a path.
3. The path is walked through root.
4. The deref expression produces the value found at that location.

Derefs are resolved at the moment the surrounding expression is evaluated. When a deref appears inside a value being constructed and stored, it resolves at storage time, not at later execution time:

```
.var.x. = 5
.var.snapshot. = (.var.x.)   // .var.snapshot. is now 5, by value
.var.x. = 99
                              // .var.snapshot. is still 5
```

To express "read the value at this path at the moment this statement runs", use deferred assignment:

```
.var.live. <- .var.x.        // resolves at execution time
```

This is the distinction between value-level dynamism (`(...)`, evaluated when its containing value is built) and statement-level dynamism (`<-`, evaluated when its containing statement runs).

---

## 7. Operations

An **operation invocation** is a map at `.ops.<name>.` with the shape:

```
.ops.<name>. = {
    args: { ... },
    return: 0,
    trigger: 0,
}
```

- `args` — a map holding the operation's named inputs.
- `return` — a slot that will hold the operation's result after firing.
- `trigger` — a slot whose *write* causes the op to fire. The stored value is unused.

### 7.1 Trigger semantics

The op `<name>` fires when one of the following writes lands:

- **A.** A write at `.ops.<name>.trigger.`, regardless of value.
- **B.** A write at `.ops.<name>.` whose new value is a map containing a `trigger` key.

Form A is useful for re-firing without rewriting args. Form B sets up and fires atomically in a single assignment.

### 7.2 Fire mechanics

When an op fires:

1. The runtime reads `.ops.<name>.args.` to obtain the current arguments.
2. The op's implementation is called with `(root, args)`.
3. The returned value is written to `.ops.<name>.return.`.

Step 3 goes through the normal assignment path, so writing the return value does not itself fire any op (the path ends in `return`, not `trigger`).

If the op fires another op transitively (for example, `run` executing a sub-program that writes a trigger), the calls nest on the host call stack.

### 7.3 Built-in operations

#### `add`

**Arguments:** `left`, `right` — both integers.
**Returns:** integer `left + right`.

```
.ops.add. = { args: { left: 5, right: 6 }, return: 0, trigger: 0 }
// .ops.add.return. is now 11
```

#### `run`

**Arguments:** `program` — a program value (see §4).
**Returns:** integer `0` as a placeholder; the operation's effect is the side effects on root.

```
.var.prog. = [[.var.x., 42, 0]]
.ops.run.args.program. <- .var.prog.
.ops.run.trigger. = 0
// .var.x. is now 42
```

`run` executes its `program` argument against the current root. All mutations made by the sub-program are immediately visible.

A sub-program may itself fire `run`. There is no halt mechanism — a sub-program runs all its statements in order, then returns. Loops are constructed by having the sub-program rewrite `.ops.run.args.program.` (typically with `<-` to pick from a set of candidate programs) and re-trigger. Runaway recursion ends in stack overflow.

---

## 8. Errors and panics

The runtime distinguishes two failure categories.

**Errors** — printed to stderr; execution of the current statement aborts; the next statement runs normally:

- Undefined key during a walk.
- Indexing into a non-map value.
- Deferred-assignment RHS that does not resolve to a valid path.

**Panics** — process aborts:

- Writing to a forbidden namespace (anything other than `.ops.` or `.var.`).
- Writing to `.ops.<name>.` where `<name>` is not in the op registry.
- Writing the empty path.
- Op arguments missing or of the wrong type.
- Statements or programs with malformed structure.
- Unknown escape sequences in literals.

---

## 9. Output

When execution finishes (or terminates with a panic), the runtime pretty-prints the final state of root to stdout:

- Integer values print as decimal numbers.
- Empty maps print as `{}`.
- Maps with keys exactly `{0, 1, …, n-1}` and all values being ASCII-printable integers (codepoints in 0x20..=0x7E) print in string form: `"hello"`.
- Maps with keys exactly `{0, 1, …, n-1}` print in array form: `[1, 2, 3]`.
- All other maps print in map form: `{k: v, k: v, …}`.

The runtime cannot distinguish a "string" from an "array of ASCII codes" because they are the same value. The printer picks the most readable form.

---

## 10. Grammar

For reference, the complete grammar in pest notation:

```
WHITESPACE = _{ " " | "\t" | "\r" | "\n" }

file = _{ SOI ~ (("lines!" ~ lines) | value) ~ EOI }

value = {
    number | list | deref
    | char_lit | string_lit | array | dot_path | string
}

dot_path = { ("." ~ value)* ~ "." }

number   = @{ "-"? ~ ASCII_DIGIT+ }
deref    = { "(" ~ value ~ ")" }
list     = { "{" ~ (list_item ~ ("," ~ list_item)* ~ ","?)? ~ "}" }
list_item = { value ~ ":" ~ value }

char_lit   = @{ "'"  ~ (("\\" ~ ANY) | (!"'"  ~ ANY))  ~ "'"  }
string_lit = @{ "\"" ~ (("\\" ~ ANY) | (!"\"" ~ ANY))* ~ "\"" }
string     = @{ ASCII_ALPHANUMERIC+ }
array      = { "[" ~ (value ~ ("," ~ value)* ~ ","?)? ~ "]" }

set_statement = { value ~ assign_op ~ value }
assign_op     = { "=" | "<-" }
lines         = { (set_statement | value)* }
```
