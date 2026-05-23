use std::collections::BTreeMap;
use std::io::Read;

use pest::Parser;
use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;

mod ops;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct MyParser;

/* =========================
   AST
   (All sugar desugars to these three forms at parse time.)
========================= */

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
pub enum Value {
    Number(i32),
    List(BTreeMap<Value, Value>),
    Deref(Box<Value>),
}


/* =========================
   AST BUILDER
========================= */

fn build_ast(mut pairs: Pairs<Rule>) -> Value {
    let got = pairs.next().unwrap();
    match got.as_rule() {
        Rule::value => return parse_value(got),
        _ => {},
    }
    let pairs = got.into_inner()
        .enumerate()
        .map(|(i, p)| (Value::Number(i as i32), parse_value_or_set(p)))
        .collect::<BTreeMap<_, _>>();
    Value::List(pairs)
}

fn parse_value_or_set(pair: Pair<Rule>) -> Value {
    match pair.as_rule() {
        Rule::set_statement => {
            let mut inner = pair.into_inner();
            let lhs = parse_value(inner.next().unwrap());
            let op  = inner.next().unwrap();
            debug_assert_eq!(op.as_rule(), Rule::assign_op);
            let depth: i32 = match op.as_str() {
                "="  => 0,
                "<-" => 1,
                other => unreachable!("unknown assign op: {}", other),
            };
            let rhs = parse_value(inner.next().unwrap());

            let mut m = BTreeMap::new();
            m.insert(Value::Number(0), lhs);
            m.insert(Value::Number(1), rhs);
            m.insert(Value::Number(2), Value::Number(depth));
            Value::List(m)
        }
        Rule::value => parse_value(pair),
        _ => unreachable!(),
    }
}

fn parse_value(pair: Pair<Rule>) -> Value {
    debug_assert_eq!(pair.as_rule(), Rule::value);
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::number => Value::Number(inner.as_str().parse::<i32>().expect("bad number")),
        Rule::char_lit => Value::Number(parse_char_lit(inner.as_str()) as i32),
        Rule::string_lit => parse_string_lit(inner.as_str()),
        Rule::array | Rule::dot_path => parse_indexed(inner.into_inner()),
        Rule::list => parse_list(inner.into_inner()),
        Rule::string => Value::List(inner.as_str().chars().enumerate().map(|(i, c)| (Value::Number(i as i32), Value::Number(c as i32))).collect()),
        Rule::deref => {
            let v = parse_value(inner.into_inner().next().unwrap());
            Value::Deref(Box::new(v))
        }
        r => unreachable!("unexpected rule in parse_value: {:?}", r),
    }
}

/// Sugar: `[a, b, c]` and `.a.b.c.` both desugar to `{0:a, 1:b, 2:c}`.
fn parse_indexed(pairs: Pairs<Rule>) -> Value {
    let mut items = BTreeMap::new();
    let mut i = 0i32;
    for p in pairs {
        debug_assert_eq!(p.as_rule(), Rule::value);
        items.insert(Value::Number(i), parse_value(p));
        i += 1;
    }
    Value::List(items)
}

fn parse_list(pairs: Pairs<Rule>) -> Value {
    let mut items = BTreeMap::new();
    for p in pairs {
        if p.as_rule() == Rule::list_item {
            let mut it = p.into_inner();
            let k = parse_value(it.next().unwrap());
            let v = parse_value(it.next().unwrap());
            items.insert(k, v);
        }
    }
    Value::List(items)
}

/// `'c'` -> codepoint.
fn parse_char_lit(s: &str) -> u32 {
    let inner = &s[1..s.len() - 1];
    decode_escaped_char(&mut inner.chars()).expect("empty char literal")
}

/// `"hello"` -> `{0:'h', 1:'e', ...}`.
fn parse_string_lit(s: &str) -> Value {
    let inner = &s[1..s.len() - 1];
    let mut items = BTreeMap::new();
    let mut chars = inner.chars();
    let mut i = 0i32;
    while let Some(code) = decode_escaped_char(&mut chars) {
        items.insert(Value::Number(i), Value::Number(code as i32));
        i += 1;
    }
    Value::List(items)
}

fn decode_escaped_char(chars: &mut std::str::Chars) -> Option<u32> {
    let c = chars.next()?;
    Some(if c == '\\' {
        match chars.next().expect("incomplete escape") {
            'n' => '\n' as u32,
            't' => '\t' as u32,
            'r' => '\r' as u32,
            '0' => '\0' as u32,
            '\\' => '\\' as u32,
            '\'' => '\'' as u32,
            '"' => '"' as u32,
            other => panic!("unknown escape: \\{}", other),
        }
    } else {
        c as u32
    })
}

/* =========================
   RUNTIME VALUES & ERRORS
========================= */

#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
enum MyValue {
    Val(i32),
    Map(BTreeMap<MyValue, MyValue>),
}

#[derive(Debug)]
enum InterpError {
    UndefinedKey(#[allow(dead_code)] MyValue),
    IndexedScalar,
    #[allow(unused)] PathNotMap,
}

/* =========================
   INTERPRETER
========================= */

fn eval(root: &MyValue, v: &Value) -> Result<MyValue, InterpError> {
    match v {
        Value::Number(n) => Ok(MyValue::Val(*n)),
        Value::List(items) => {
            let mut map = BTreeMap::new();
            for (k, v) in items {
                map.insert(eval(root, k)?, eval(root, v)?);
            }
            Ok(MyValue::Map(map))
        }
        Value::Deref(inner) => {
            let target = eval(root, inner)?;
            walk(root, iterate(&target)).cloned()
        }
    }
}

fn iterate(x: &MyValue) -> impl Iterator<Item = &MyValue> {
    let MyValue::Map(m) = x else {
        panic!();
    };
    let mut i = 0;
    std::iter::from_fn(move || {
        let got = m.get(&MyValue::Val(i));
        i += 1;
        got
    })
}

fn walk<'a, 'b>(root: &'a MyValue, path: impl Iterator<Item = &'b MyValue>) -> Result<&'a MyValue, InterpError> {
    let mut current = root;
    for key in path {
        let MyValue::Map(m) = current else {
            return Err(InterpError::IndexedScalar);
        };
        current = m
            .get(key)
            .ok_or_else(|| InterpError::UndefinedKey(key.clone()))?;
    }
    Ok(current)
}

fn assign(root: &mut MyValue, path: &MyValue, val: MyValue) -> Result<(), InterpError> {
    let components: Vec<&MyValue> = iterate(path).collect();

    if components.is_empty() {
        panic!("cannot replace root: writes must go through a namespace");
    }

    check_write_path(&components);

    let mut current = &mut *root;
    for key in &components {
        if !matches!(current, MyValue::Map(_)) {
            *current = MyValue::Map(BTreeMap::new());
        }
        let MyValue::Map(m) = current else { unreachable!() };
        current = m.entry((*key).clone()).or_insert(MyValue::Val(0));
    }
    *current = val;

    // Re-borrow the just-written value to inspect it.
    let written = walk(root, components.iter().copied())?.clone();
    maybe_fire(root, &components, &written)
}

fn check_write_path(components: &[&MyValue]) {
    let first = myvalue_as_string(components[0])
        .unwrap_or_else(|| panic!("first path component must be a string namespace"));
    match first.as_str() {
        "ops" => {
            if components.len() >= 2 {
                let opname = myvalue_as_string(components[1])
                    .unwrap_or_else(|| panic!("ops name must be a string"));
                if ops::op_registry(&opname).is_none() {
                    panic!("unknown op: '{}'", opname);
                }
            }
        }
        "var" => {}
        other => panic!("forbidden namespace: '{}' (allowed: ops, var)", other),
    }
}

fn maybe_fire(
    root: &mut MyValue,
    components: &[&MyValue],
    written: &MyValue,
) -> Result<(), InterpError> {
    // Must be inside .ops.<name>. at minimum.
    if components.len() < 2 { return Ok(()); }
    if myvalue_as_string(components[0]).as_deref() != Some("ops") { return Ok(()); }
    let Some(opname) = myvalue_as_string(components[1]) else { return Ok(()); };

    // Case A: path contains .ops.<name>.trigger.
    if components.len() >= 3
        && myvalue_as_string(components[2]).as_deref() == Some("trigger")
    {
        return fire_op(root, &opname);
    }

    // Case B: path ends at .ops.<name>. and written value contains a trigger key.
    if components.len() == 2 {
        if let MyValue::Map(m) = written {
            if m.contains_key(&str_to_myvalue("trigger")) {
                return fire_op(root, &opname);
            }
        }
    }

    Ok(())
}

fn fire_op(root: &mut MyValue, opname: &str) -> Result<(), InterpError> {
    // Namespace check guarantees the op is registered, so unwrap is safe.
    let op_fn = ops::op_registry(opname).unwrap();

    let args_path = build_path(&["ops", opname, "args"]);
    let args = walk(root, iterate(&args_path))?.clone();

    let result = op_fn(&args)?;

    let return_path = build_path(&["ops", opname, "return"]);
    assign(root, &return_path, result)
}

fn build_path(components: &[&str]) -> MyValue {
    let mut m = BTreeMap::new();
    for (i, s) in components.iter().enumerate() {
        m.insert(MyValue::Val(i as i32), str_to_myvalue(s));
    }
    MyValue::Map(m)
}

fn str_to_myvalue(s: &str) -> MyValue {
    let mut m = BTreeMap::new();
    for (i, c) in s.chars().enumerate() {
        m.insert(MyValue::Val(i as i32), MyValue::Val(c as i32));
    }
    MyValue::Map(m)
}

fn myvalue_as_string(v: &MyValue) -> Option<String> {
    let MyValue::Map(m) = v else { return None; };
    let mut s = String::new();
    for i in 0i32..(m.len() as i32) {
        let MyValue::Val(code) = m.get(&MyValue::Val(i))? else { return None; };
        s.push(char::from_u32(*code as u32)?);
    }
    Some(s)
}

fn exec(root: &mut MyValue, instructions: &Value) -> Result<(), InterpError> {
    let mut i = 0;
    let Value::List(list) = instructions else { panic!(); };
    while let Some(val) = list.get(&Value::Number(i)) {
        let Value::List(stmt) = val else { panic!(); };
        let Some(a) = stmt.get(&Value::Number(0)) else { panic!(); };
        let Some(b) = stmt.get(&Value::Number(1)) else { panic!(); };
        let depth = match stmt.get(&Value::Number(2)) {
            Some(Value::Number(n)) => *n,
            None => 0, // tolerate stored programs without the slot
            _ => panic!("statement depth must be a number"),
        };

        if let Err(e) = do_set(root, a, b, depth) {
            eprintln!("runtime error in {:?}: {:?}", val, e);
        }
        i += 1;
    }
    Ok(())
}

fn do_set(root: &mut MyValue, from: &Value, to: &Value, depth: i32) -> Result<(), InterpError> {
    let from = eval(root, from)?;       // LHS: evaluate once, treat as path. Unchanged.
    let mut to = eval(root, to)?;       // RHS: evaluate once (existing behavior).

    // For depth N, follow the path on the RHS N additional times.
    // depth=0 → eager, no extra follow.
    // depth=1 → `<-`, one extra follow.
    // depth≥2 → reserved for the generalization you might keep open.
    for _ in 0..depth {
        to = walk(root, iterate(&to))?.clone();
    }

    assign(root, &from, to)
}

/* =========================
   PRETTY PRINTING

   The runtime can't distinguish "{0:'h',1:'i'}" from "\"hi\"" from "[104,105]"
   — they're all the same value. We pick the prettiest representation:
     - 0..n-1 keys + all values ASCII-printable  -> string  "hi"
     - 0..n-1 keys                               -> array   [104, 105]
     - else                                      -> map     {k:v, ...}
========================= */

fn print_value(v: &MyValue) {
    match v {
        MyValue::Val(n) => print!("{}", n),
        MyValue::Map(m) => {
            if m.is_empty() {
                print!("{{}}");
                return;
            }
            if is_indexed(m) {
                if let Some(s) = try_as_string(m) {
                    print!("{:?}", s); // Rust's debug-quoting handles escapes
                    return;
                }
                print!("[");
                for i in 0i32..(m.len() as i32) {
                    if i > 0 {
                        print!(", ");
                    }
                    print_value(&m[&MyValue::Val(i)]);
                }
                print!("]");
                return;
            }
            print!("{{");
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    print!(", ");
                }
                print_value(k);
                print!(": ");
                print_value(v);
            }
            print!("}}");
        }
    }
}

fn is_indexed(m: &BTreeMap<MyValue, MyValue>) -> bool {
    (0i32..(m.len() as i32)).all(|i| m.contains_key(&MyValue::Val(i)))
}

fn try_as_string(m: &BTreeMap<MyValue, MyValue>) -> Option<String> {
    let mut s = String::new();
    for i in 0i32..(m.len() as i32) {
        let MyValue::Val(code) = m.get(&MyValue::Val(i))? else {
            return None;
        };
        if !(0x20..=0x7E).contains(code) {
            return None;
        }
        s.push(*code as u8 as char);
    }
    Some(s)
}

/* =========================
   MAIN
========================= */

fn read_source() -> String {
    if let Some(path) = std::env::args().nth(1) {
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("could not read {}: {}", path, e);
            std::process::exit(1);
        })
    } else {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .unwrap_or_else(|e| {
                eprintln!("could not read stdin: {}", e);
                std::process::exit(1);
            });
        s
    }
}

fn main() {
    let src = read_source();
    let ast = build_ast(
        MyParser::parse(Rule::file, &src)
            .unwrap_or_else(|e| panic!("parse error: {e}")),
    );

    let mut root = MyValue::Val(0);
    exec(&mut root, &ast).unwrap();
    print_value(&root);
}
