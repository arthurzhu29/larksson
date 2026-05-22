use std::collections::BTreeMap;

use pest::Parser;
use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct MyParser;

/* =========================
   AST
========================= */

#[derive(Debug, Clone)]
pub enum Value {
    Number(u32),
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
        Rule::number => Value::Number(inner.as_str().parse().unwrap()),
        Rule::list => parse_list(inner.into_inner()),
        Rule::deref => {
            let v = parse_value(inner.into_inner().next().unwrap());
            Value::Deref(Box::new(v))
        }
        r => unreachable!("unexpected rule in parse_value: {:?}", r),
    }
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

/* =========================
   RUNTIME VALUES & ERRORS
========================= */

#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
enum MyValue {
    Val(u32),
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

/// Evaluate a Value to a concrete MyValue (no path interpretation).
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
            // Evaluate the inner value, treat the result as a path, look up in root.
            let target = eval(root, inner)?;
            let path = as_path(&target)?;
            walk(root, &path).cloned()
        }
    }
}

/// Interpret a MyValue as a path: take entries at integer keys 0, 1, 2, ...
/// in order, stopping when the next key is missing.
///
/// Examples:
///   {0:5}            -> [5]
///   {0:1, 1:3}       -> [1, 3]
///   {}               -> []          (empty path = root itself)
///   {0:1, 2:9}       -> [1]         (1 is missing; 9 is silently ignored)
fn as_path(v: &MyValue) -> Result<Vec<MyValue>, InterpError> {
    let MyValue::Map(m) = v else {
        return Err(InterpError::PathNotMap);
    };
    let mut path = Vec::with_capacity(m.len());
    for i in 0u32.. {
        match m.get(&MyValue::Val(i)) {
            Some(c) => path.push(c.clone()),
            None => break,
        }
    }
    Ok(path)
}

/// Walk root along path components (read-only).
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

/// Walk root along path components, creating intermediate maps as needed,
/// and assign `val` at the end. Empty path replaces root.
fn assign(root: &mut MyValue, path: &[MyValue], val: MyValue) -> Result<(), InterpError> {
    if path.is_empty() {
        *root = val;
        return Ok(());
    }
    let mut current = root;
    for key in path {
        // Walking through a scalar destructively replaces it with a fresh map,
        // same policy as before.
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
            println!("{:?}", v);
        }
        Statement::Set { lhs, rhs } => {
            // LHS evaluated as a value, then reinterpreted as a path.
            let lhs_val = eval(root, lhs)?;
            let path = as_path(&lhs_val)?;
            let rhs_val = eval(root, rhs)?;
            assign(root, &path, rhs_val)?;
        }
    }
    Ok(())
}

/* =========================
   MAIN
========================= */

// Translation of the old INPUT:
//   .2. = 3                       ->  {0:2} = 3
//   .1.(.2.). = {1:2, 3:4}        ->  {0:1, 1:({0:2})} = {1:2, 3:4}
//   print .1.                     ->  print ({0:1})
const INPUT: &str = r#"
{0:2} = 3
{0:1, 1:({0:2})} = {1:2, 3:4}
print ({0:1})
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