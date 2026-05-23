# &lt;name, formerly larksson&gt;

A small homoiconic language: programs and data have the same shape because they are the same thing.

```
lines!
.ops.add. = { args: { left: 2, right: 3 }, return: 0, trigger: 0 }
.var.sum. = (.ops.add.return.)
```

That runs `2 + 3`, stores `5` in `.var.sum.`, and dumps the final state of root. Three things to notice: `add` is a value with a particular shape, the assignment to a key called `trigger` is what causes computation to happen, and the result is read back through a path. Every operation works this way.

## The conceit

There are exactly two kinds of values: integers and maps. Strings are maps from indices to character codes. Arrays are maps from indices to their elements. Paths are maps from indices to their components. Programs are maps from indices to statements. Statements are maps from indices to their parts. There is no notion of "expression" separate from "data" because the language doesn't have one — there's just data, and a few places where the runtime interprets it.

This pays off when you want to construct a program at runtime and execute it. You build it as a map. You hand it to `run`. That is the whole story.

## Running it

```
cargo run -- sample.larks
```

Reads from the given file, or from stdin if no path is given. The final state of root prints to stdout when the program finishes.

## More

The full reference is in [REFERENCE.md](REFERENCE.md). Start there if you want to write something.
