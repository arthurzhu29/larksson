use std::collections::BTreeMap;

use pest::Parser;
use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct MyParser;

/* =========================
   AST
   (All sugar desugars to these three forms at parse time.)
========================= */

#[derive(Debug, Clone)]
pub enum Value {
    Number(i32),
    List(Vec<(Value, Value)>),
    Deref(Box<Value>),
}

#[derive(Debug)]
pub enum Statement {
    Set { lhs: Value, rhs: Value },
    Print { value: Value },
}

/* =========================
   AST BUILDER
========================= */

fn build_ast(pairs: Pairs<Rule>) -> Vec<Statement> {
    pairs
        .filter_map(|p| match p.as_rule() {
            Rule::statement => Some(parse_statement(p.into_inner())),
            Rule::EOI => None,
            r => unreachable!("unexpected top-level rule: {:?}", r),
        })
        .collect()
}

fn parse_statement(mut pairs: Pairs<Rule>) -> Statement {
    let pair = pairs.next().unwrap();
    match pair.as_rule() {
        Rule::set_statement => {
            let mut inner = pair.into_inner();
            let lhs = parse_value(inner.next().unwrap());
            let rhs = parse_value(inner.next().unwrap());
            Statement::Set { lhs, rhs }
        }
        Rule::print_statement => {
            let value = parse_value(pair.into_inner().next().unwrap());
            Statement::Print { value }
        }
        r => unreachable!("unexpected rule in parse_statement: {:?}", r),
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
        Rule::deref => {
            let v = parse_value(inner.into_inner().next().unwrap());
            Value::Deref(Box::new(v))
        }
        r => unreachable!("unexpected rule in parse_value: {:?}", r),
    }
}

/// Sugar: `[a, b, c]` and `.a.b.c.` both desugar to `{0:a, 1:b, 2:c}`.
fn parse_indexed(pairs: Pairs<Rule>) -> Value {
    let mut items = vec![];
    let mut i = 0i32;
    for p in pairs {
        debug_assert_eq!(p.as_rule(), Rule::value);
        items.push((Value::Number(i), parse_value(p)));
        i += 1;
    }
    Value::List(items)
}

fn parse_list(pairs: Pairs<Rule>) -> Value {
    let mut items = vec![];
    for p in pairs {
        if p.as_rule() == Rule::list_item {
            let mut it = p.into_inner();
            let k = parse_value(it.next().unwrap());
            let v = parse_value(it.next().unwrap());
            items.push((k, v));
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
    let mut items = vec![];
    let mut chars = inner.chars();
    let mut i = 0i32;
    while let Some(code) = decode_escaped_char(&mut chars) {
        items.push((Value::Number(i), Value::Number(code as i32)));
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
    PathNotMap,
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
            let path = as_path(&target)?;
            walk(root, &path).cloned()
        }
    }
}

/// A path is a value's entries at integer keys 0, 1, 2, ... taken in order
/// until the next key is missing.
fn as_path(v: &MyValue) -> Result<Vec<MyValue>, InterpError> {
    let MyValue::Map(m) = v else {
        return Err(InterpError::PathNotMap);
    };
    let mut path = Vec::with_capacity(m.len());
    for i in 0i32.. {
        match m.get(&MyValue::Val(i)) {
            Some(c) => path.push(c.clone()),
            None => break,
        }
    }
    Ok(path)
}

fn walk<'a>(root: &'a MyValue, path: &[MyValue]) -> Result<&'a MyValue, InterpError> {
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

fn assign(root: &mut MyValue, path: &[MyValue], val: MyValue) -> Result<(), InterpError> {
    if path.is_empty() {
        *root = val;
        return Ok(());
    }
    let mut current = root;
    for key in path {
        if !matches!(current, MyValue::Map(_)) {
            *current = MyValue::Map(BTreeMap::new());
        }
        let MyValue::Map(m) = current else { unreachable!() };
        current = m.entry(key.clone()).or_insert(MyValue::Val(0));
    }
    *current = val;
    Ok(())
}

fn exec(root: &mut MyValue, stmt: &Statement) -> Result<(), InterpError> {
    match stmt {
        Statement::Print { value } => {
            let v = eval(root, value)?;
            print_value(&v);
            println!();
        }
        Statement::Set { lhs, rhs } => {
            let lhs_val = eval(root, lhs)?;
            let path = as_path(&lhs_val)?;
            let rhs_val = eval(root, rhs)?;
            assign(root, &path, rhs_val)?;
        }
    }
    Ok(())
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

const INPUT: &str = r#"
.2. = 3
.1.(.2.). = {1:2, 3:4}
print (.1.)
print "hello"
print 'A'
print [-5, 0, 5]
print [104, 101, 108, 108, 111]
"#;

fn main() {
    let ast = build_ast(
        MyParser::parse(Rule::file, INPUT)
            .unwrap_or_else(|e| panic!("parse error: {e}")),
    );

    let mut root = MyValue::Val(0);
    for stmt in &ast {
        if let Err(e) = exec(&mut root, stmt) {
            eprintln!("runtime error in {:?}: {:?}", stmt, e);
        }
    }
}
