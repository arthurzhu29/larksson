use std::collections::HashMap;
use std::hash::{Hash, Hasher};

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
    let mut out = vec![];

    for pair in pairs {
        match pair.as_rule() {
            Rule::statement => {
                out.push(parse_statement(pair.into_inner()));
            }
            _ => {}
        }
    }

    out
}

/* =========================
   STATEMENTS
========================= */

fn parse_statement(mut pairs: Pairs<Rule>) -> Statement {
    let pair = pairs.next().unwrap();

    match pair.as_rule() {
        Rule::set_statement => parse_set(pair.into_inner()),
        Rule::print_statement => parse_print(pair.into_inner()),
        _ => unreachable!(),
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

        _ => unreachable!(),
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
   MAIN
========================= */


const INPUT: &str = r#"
.2. = 3
.1.(.2.). = {1:2, 3:4}
print .1.
"#;

fn main() {

    let ast = build_ast(
        MyParser::parse(Rule::file, INPUT).unwrap()
    );

    let mut root = MyValue::Val(0);
    for stmt in &ast {
        apply_to_root(&mut root, stmt);
    }
}

fn apply_to_root(root: &mut MyValue, stmt: &Statement) {
    match stmt {
        Statement::Print { var } => println!("{:?}", resolve_var(root, &var.0)),
        Statement::Set { var, value } => {
            if let Some(val) = resolve_value(root, value) {
                set_var(root, &var.0, val);
            }
        },
    }
}

fn resolve_value(root: &MyValue, value: &Value) -> Option<MyValue> {
    match value {
        Value::Number(n) => Some(MyValue::Val(*n)),
        Value::Var(v) => resolve_var(root, &v.0).cloned(),
        Value::List(list) => Some(MyValue::Map(
            list.iter()
                .map(
                    |(v1, v2)|
                    resolve_value(root, v1).zip(resolve_value(root, v2))
                ).collect::<Option<_>>()?
        )),
    }
}

fn resolve_var<'a>(root: &'a MyValue, path: &[Value]) -> Option<&'a MyValue> {
    let mut current = root;
    for val in path {
        let MyValue::Map(map) = current else {
            return None;
        };
        let my_val = resolve_value(root, val)?;
        current = map.get(&my_val)?;
    }
    Some(current)
}

fn set_var<'a>(root: &'a mut MyValue, path: &[Value], val: MyValue) {
    let clone = root.clone();
    let mut current = root;
    for val in path {
        if !matches!(current, MyValue::Map(_)) {
            *current = MyValue::Map(HashMap::new());
        }
        let MyValue::Map(map) = current else {
            unreachable!();
        };
        let Some(my_val) = resolve_value(&clone, val) else {
            return;
        };
        current = map.entry(my_val).or_insert(MyValue::Val(0));
    }
    *current = val;
}

#[derive(Eq, Clone, Debug, PartialEq)]
enum MyValue {
    Val(u32),
    Map(HashMap<MyValue, MyValue>),
}

impl Hash for MyValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            MyValue::Val(v) => {
                0u8.hash(state);
                v.hash(state);
            }

            MyValue::Map(map) => {
                1u8.hash(state);

                // IMPORTANT: make order deterministic
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));

                for (k, v) in entries {
                    k.hash(state);
                    v.hash(state);
                }
            }
        }
    }
}

use std::cmp::Ordering;


/* =========================
   PARTIAL ORD
========================= */

impl PartialOrd for MyValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/* =========================
   ORD
========================= */

impl Ord for MyValue {
    fn cmp(&self, other: &Self) -> Ordering {
        use MyValue::*;

        match (self, other) {

            /* -------------------------
               Val vs Val
            ------------------------- */
            (Val(a), Val(b)) => a.cmp(b),

            /* -------------------------
               Val vs Map
               (define ordering rule: Val < Map)
            ------------------------- */
            (Val(_), Map(_)) => Ordering::Less,
            (Map(_), Val(_)) => Ordering::Greater,

            /* -------------------------
               Map vs Map
               compare lexicographically
            ------------------------- */
            (Map(a), Map(b)) => {
                let len_cmp = a.len().cmp(&b.len());
                if len_cmp != Ordering::Equal {
                    return len_cmp;
                }

                let (mut a, mut b) = (a.iter().collect::<Vec<_>>(), b.iter().collect::<Vec<_>>());
                a.sort_by(|a, b| a.0.cmp(b.0));
                b.sort_by(|a, b| a.0.cmp(b.0));

                for ((ak, av), (bk, bv)) in a.iter().zip(b.iter()) {
                    let k_cmp = ak.cmp(bk);
                    if k_cmp != Ordering::Equal {
                        return k_cmp;
                    }

                    let v_cmp = av.cmp(bv);
                    if v_cmp != Ordering::Equal {
                        return v_cmp;
                    }
                }

                Ordering::Equal
            }
        }
    }
}