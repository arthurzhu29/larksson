use std::collections::BTreeMap;

use pest::Parser;
use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct MyParser;

/* =========================
   AST DEFINITIONS
========================= */

#[derive(Debug)]
pub enum Statement {
    Set { var: Var, value: Value },
    Print { var: Var },
}

#[derive(Debug)]
pub enum Value {
    Number(u32),
    Var(Var),
    List(Vec<(Value, Value)>),
}

#[derive(Debug)]
pub struct Var(pub Vec<Value>);

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

/* =========================
   STATEMENTS
========================= */

fn parse_statement(mut pairs: Pairs<Rule>) -> Statement {
    let pair = pairs.next().unwrap();

    match pair.as_rule() {
        Rule::set_statement => parse_set(pair.into_inner()),
        Rule::print_statement => parse_print(pair.into_inner()),
        r => unreachable!("unexpected rule in parse_statement: {:?}", r),
    }
}

fn parse_set(mut pairs: Pairs<Rule>) -> Statement {
    let var = parse_var(pairs.next().unwrap());
    let value = parse_value(pairs.next().unwrap());

    Statement::Set { var, value }
}

fn parse_print(mut pairs: Pairs<Rule>) -> Statement {
    let var = parse_var(pairs.next().unwrap());

    Statement::Print { var }
}

/* =========================
   VALUES
========================= */

fn parse_value(pair: Pair<Rule>) -> Value {
    match pair.as_rule() {
        Rule::number => Value::Number(pair.as_str().parse().unwrap()),

        Rule::closed_var => {
            let inner = pair.into_inner().next().unwrap();
            Value::Var(parse_var(inner))
        }

        Rule::list => parse_list(pair.into_inner()),

        r => unreachable!("unexpected rule in parse_value: {:?}", r),
    }
}

/* =========================
   VAR
========================= */

fn parse_var(pair: Pair<Rule>) -> Var {
    let mut parts = vec![];

    for inner in pair.into_inner() {
        parts.push(parse_value(inner));
    }

    Var(parts)
}

/* =========================
   LIST
========================= */

fn parse_list(pairs: Pairs<Rule>) -> Value {
    let mut items = vec![];

    for pair in pairs {
        if pair.as_rule() == Rule::list_item {
            let mut inner = pair.into_inner();

            let key = parse_value(inner.next().unwrap());
            let value = parse_value(inner.next().unwrap());

            items.push((key, value));
        }
    }

    Value::List(items)
}

/* =========================
   INTERPRETER VALUES & ERRORS
========================= */

// BTreeMap gives us deterministic iteration order, which lets us derive
// Hash and Ord directly instead of hand-rolling them.
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
enum MyValue {
    Val(u32),
    Map(BTreeMap<MyValue, MyValue>),
}

#[derive(Debug)]
enum InterpError {
    UndefinedKey(#[allow(dead_code)] MyValue),
    IndexedScalar,
}

/* =========================
   INTERPRETER
========================= */

fn apply_to_root(root: &mut MyValue, stmt: &Statement) -> Result<(), InterpError> {
    match stmt {
        Statement::Print { var } => {
            let val = resolve_var(root, &var.0)?;
            println!("{:?}", val);
        }
        Statement::Set { var, value } => {
            let val = resolve_value(root, value)?;
            set_var(root, &var.0, val)?;
        }
    }
    Ok(())
}

fn resolve_value(root: &MyValue, value: &Value) -> Result<MyValue, InterpError> {
    match value {
        Value::Number(n) => Ok(MyValue::Val(*n)),
        Value::Var(v) => resolve_var(root, &v.0).cloned(),
        Value::List(list) => {
            let entries: Result<BTreeMap<_, _>, _> = list
                .iter()
                .map(|(k, v)| Ok((resolve_value(root, k)?, resolve_value(root, v)?)))
                .collect();
            Ok(MyValue::Map(entries?))
        }
    }
}

fn resolve_var<'a>(root: &'a MyValue, path: &[Value]) -> Result<&'a MyValue, InterpError> {
    let mut current = root;
    for val in path {
        let MyValue::Map(map) = current else {
            return Err(InterpError::IndexedScalar);
        };
        let key = resolve_value(root, val)?;
        current = map.get(&key).ok_or(InterpError::UndefinedKey(key))?;
    }
    Ok(current)
}

fn set_var(root: &mut MyValue, path: &[Value], val: MyValue) -> Result<(), InterpError> {
    // Resolve every path component against the current state up front, so the
    // mutable traversal below doesn't need to re-borrow `root`. This replaces
    // the previous full-tree clone with an O(path) walk.
    let keys: Vec<MyValue> = path
        .iter()
        .map(|v| resolve_value(root, v))
        .collect::<Result<_, _>>()?;

    let mut current = root;
    for key in keys {
        // Walking through a scalar destructively replaces it with a fresh map.
        // This is intentional: `.1. = 5` followed by `.1.(.2.). = 3` overwrites
        // the scalar 5 with the new nested structure.
        if !matches!(current, MyValue::Map(_)) {
            *current = MyValue::Map(BTreeMap::new());
        }
        let MyValue::Map(map) = current else { unreachable!() };
        current = map.entry(key).or_insert(MyValue::Val(0));
    }
    *current = val;
    Ok(())
}

/* =========================
   MAIN
========================= */

const INPUT: &str = r#"
.2. = 3
.1.(.2.). = {1:2, 3:4}
print .1.
"#;

fn main() {
    let ast = build_ast(
        MyParser::parse(Rule::file, INPUT)
            .unwrap_or_else(|e| panic!("parse error: {e}")),
    );

    let mut root = MyValue::Val(0);
    for stmt in &ast {
        if let Err(e) = apply_to_root(&mut root, stmt) {
            eprintln!("runtime error in {:?}: {:?}", stmt, e);
        }
    }
}