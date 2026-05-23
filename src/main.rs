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
========================= */

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
pub enum Value {
    Number(i32),
    List(BTreeMap<Value, Value>),
    Deref(Box<Value>),
    SelfSentinel,
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

/// Statement shape: [[lhs, lhs_depth], [rhs, rhs_depth]].
///   `a = b`  -> [[a, 0], [b, 0]]
///   `a <- b` -> [[a, 0], [b, 1]]
///   `a -> b` -> [[a, 1], [b, 0]]
fn parse_value_or_set(pair: Pair<Rule>) -> Value {
    match pair.as_rule() {
        Rule::set_statement => {
            let mut inner = pair.into_inner();
            let lhs = parse_value(inner.next().unwrap());
            let op  = inner.next().unwrap();
            debug_assert_eq!(op.as_rule(), Rule::assign_op);
            let (lhs_d, rhs_d): (i32, i32) = match op.as_str() {
                "="  => (0, 0),
                "<-" => (0, 1),
                "->" => (1, 0),
                other => unreachable!("unknown assign op: {}", other),
            };
            let rhs = parse_value(inner.next().unwrap());

            Value::List(BTreeMap::from([
                (Value::Number(0), Value::List(BTreeMap::from([
                    (Value::Number(0), lhs),
                    (Value::Number(1), Value::Number(lhs_d)),
                ]))),
                (Value::Number(1), Value::List(BTreeMap::from([
                    (Value::Number(0), rhs),
                    (Value::Number(1), Value::Number(rhs_d)),
                ]))),
            ]))
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
        Rule::self_lit => Value::SelfSentinel,
        Rule::deref => {
            let v = parse_value(inner.into_inner().next().unwrap());
            Value::Deref(Box::new(v))
        }
        r => unreachable!("unexpected rule in parse_value: {:?}", r),
    }
}

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

fn parse_char_lit(s: &str) -> u32 {
    let inner = &s[1..s.len() - 1];
    decode_escaped_char(&mut inner.chars()).expect("empty char literal")
}

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
pub enum MyValue {
    /// The self sentinel. Sorts before all other values (declared first).
    /// Produced by reads of missing keys; never appears in stored data
    /// (writes-of-self are filtered or remove the key).
    Self_,
    /// Native integer storage. Conceptually equivalent to a unary-shape map
    /// (see `myvalue_to_num` / `num_to_myvalue` for the conversion);
    /// kept as a fast representation that ops convert from/to.
    Val(i32),
    Map(BTreeMap<MyValue, MyValue>),
}

static SELF_VALUE: MyValue = MyValue::Self_;
pub(crate) fn self_ref<'a>() -> &'a MyValue { &SELF_VALUE }

#[derive(Debug)]
pub enum InterpError {
    MalformedStatement,
}

/* =========================
   NUMBER ENCODING
   Conceptually, every number is a map:
     0 = {}
     1 = {{}: {}}
     2 = {{{}: {}}: {}}
     n = {n-1: {}}
   We store as Val(i32) for speed. Convert at boundaries only.
========================= */

pub(crate) fn num_to_myvalue(n: i32) -> MyValue {
    if n < 0 {
        panic!("negative numbers not representable in unary encoding: {}", n);
    }
    // Iterative to avoid stack overflow on large n.
    let mut current = MyValue::Map(BTreeMap::new());
    for _ in 0..n {
        let mut m = BTreeMap::new();
        m.insert(current, MyValue::Map(BTreeMap::new()));
        current = MyValue::Map(m);
    }
    current
}

/// Returns Some(n) if v is either Val(n) or a canonical unary-form Map of n.
pub(crate) fn myvalue_to_num(v: &MyValue) -> Option<i32> {
    if let MyValue::Val(n) = v {
        return Some(*n);
    }
    let mut count = 0i32;
    let mut current = v;
    loop {
        let MyValue::Map(m) = current else { return None; };
        if m.is_empty() {
            return Some(count);
        }
        if m.len() != 1 {
            return None;
        }
        let (key, val) = m.iter().next().unwrap();
        // Value must be Map(empty), i.e., 0 in unary.
        match val {
            MyValue::Map(vm) if vm.is_empty() => {}
            _ => return None,
        }
        current = key;
        count = count.checked_add(1)?;
    }
}

/* =========================
   EVAL
========================= */

fn eval(root: &MyValue, v: &Value) -> Result<MyValue, InterpError> {
    match v {
        Value::Number(n) => Ok(MyValue::Val(*n)),
        Value::SelfSentinel => Ok(MyValue::Self_),
        Value::List(items) => {
            let mut map = BTreeMap::new();
            for (k, v) in items {
                let key = eval(root, k)?;
                let val = eval(root, v)?;
                // Self-valued entries are filtered at construction; storing
                // self anywhere is illegal.
                if matches!(val, MyValue::Self_) {
                    continue;
                }
                map.insert(key, val);
            }
            Ok(MyValue::Map(map))
        }
        Value::Deref(inner) => {
            let target = eval(root, inner)?;
            walk(root, iterate(&target)).cloned()
        }
    }
}

/* =========================
   PATH MACHINERY
========================= */

fn iterate(x: &MyValue) -> impl Iterator<Item = &MyValue> {
    let m = if let MyValue::Map(m) = x { Some(m) } else { None };
    let mut i = 0i32;
    std::iter::from_fn(move || {
        let m = m?;
        let got = m.get(&MyValue::Val(i));
        i += 1;
        got
    })
}

fn walk<'a, 'b>(root: &'a MyValue, path: impl Iterator<Item = &'b MyValue>) -> Result<&'a MyValue, InterpError> {
    let mut current = root;
    for key in path {
        match current {
            MyValue::Map(m) => {
                current = m.get(key).unwrap_or(self_ref());
            }
            MyValue::Self_ | MyValue::Val(_) => {
                // Indexing into self yields self (self acts like {}).
                // Indexing into a number conceptually materializes its unary form;
                // the only key that would match in that form is num_to_myvalue(n-1),
                // a deeply-nested Map that no realistic path component equals.
                // So lookups effectively always miss: return self.
                current = self_ref();
            }
        }
    }
    Ok(current)
}

/* =========================
   ASSIGNMENT
========================= */

fn assign(root: &mut MyValue, path: &MyValue, val: MyValue) -> Result<(), InterpError> {
    let components: Vec<&MyValue> = iterate(path).collect();

    if components.is_empty() {
        panic!("cannot replace root: writes must go through a namespace");
    }

    check_write_path(&components);
    check_write_value(&components, &val);

    // Writing self removes the keyed entry.
    if matches!(val, MyValue::Self_) {
        remove_at(root, &components);
        return Ok(());
    }

    // Walk to the target slot, creating intermediate maps as needed.
    let mut current = &mut *root;
    for key in &components {
        if !matches!(current, MyValue::Map(_)) {
            *current = MyValue::Map(BTreeMap::new());
        }
        let MyValue::Map(m) = current else { unreachable!() };
        current = m.entry((*key).clone()).or_insert_with(|| MyValue::Map(BTreeMap::new()));
    }
    *current = val;

    let written = walk(root, components.iter().copied())?.clone();
    maybe_fire(root, &components, &written)
}

fn remove_at(root: &mut MyValue, components: &[&MyValue]) {
    if components.is_empty() {
        return;
    }
    let mut current = &mut *root;
    for key in &components[..components.len() - 1] {
        match current {
            MyValue::Map(m) => {
                if let Some(next) = m.get_mut(*key) {
                    current = next;
                } else {
                    return;
                }
            }
            _ => return,
        }
    }
    if let MyValue::Map(m) = current {
        m.remove(components[components.len() - 1]);
    }
}

/* =========================
   NAMESPACE AND TRIGGER ENFORCEMENT
========================= */

fn check_write_path(components: &[&MyValue]) {
    let first = myvalue_as_string(components[0])
        .unwrap_or_else(|| panic!("first path component must be a string namespace"));
    match first.as_str() {
        "ops" => {
            if components.len() < 2 {
                panic!("cannot write to .ops. directly");
            }
            let opname = myvalue_as_string(components[1])
                .unwrap_or_else(|| panic!("ops name must be a string"));
            if ops::op_registry(&opname).is_none() {
                panic!("unknown op: '{}'", opname);
            }
        }
        "var" => {}
        other => panic!("forbidden namespace: '{}' (allowed: ops, var)", other),
    }
}

fn check_write_value(components: &[&MyValue], val: &MyValue) {
    if myvalue_as_string(components[0]).as_deref() != Some("ops") {
        return;
    }

    // Removing .ops.<op>. or .ops.<op>.trigger. is illegal (kills the trigger slot).
    if matches!(val, MyValue::Self_) {
        if components.len() == 2 {
            panic!("cannot delete .ops.<op>. (would remove trigger field)");
        }
        if components.len() == 3
            && myvalue_as_string(components[2]).as_deref() == Some("trigger")
        {
            panic!("cannot remove trigger field");
        }
        return;
    }

    // Writing to .ops.<op>.trigger. directly: must be zero.
    if components.len() == 3
        && myvalue_as_string(components[2]).as_deref() == Some("trigger")
    {
        validate_trigger_value(val);
        return;
    }

    // Writing the whole op record: if it contains a trigger key, validate that.
    if components.len() == 2 {
        if let MyValue::Map(m) = val {
            if let Some(trigger_val) = m.get(&str_to_myvalue("trigger")) {
                validate_trigger_value(trigger_val);
            }
        }
    }
}

fn validate_trigger_value(val: &MyValue) {
    // Accept either representation of zero: Val(0) or Map(empty).
    match val {
        MyValue::Val(0) => {}
        MyValue::Map(m) if m.is_empty() => {}
        _ => panic!("trigger may only be set to 0 or {{}}, got {:?}", val),
    }
}

/* =========================
   TRIGGER FIRING
========================= */

fn maybe_fire(
    root: &mut MyValue,
    components: &[&MyValue],
    written: &MyValue,
) -> Result<(), InterpError> {
    if components.len() < 2 { return Ok(()); }
    if myvalue_as_string(components[0]).as_deref() != Some("ops") { return Ok(()); }
    let Some(opname) = myvalue_as_string(components[1]) else { return Ok(()); };

    if components.len() >= 3
        && myvalue_as_string(components[2]).as_deref() == Some("trigger")
    {
        return fire_op(root, &opname);
    }

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
    let op_fn = ops::op_registry(opname).unwrap();
    let args_path = build_path(&["ops", opname, "args"]);
    let args = walk(root, iterate(&args_path))?.clone();
    let result = op_fn(root, &args)?;
    let return_path = build_path(&["ops", opname, "return"]);
    assign(root, &return_path, result)
}

/* =========================
   STATEMENT EXECUTION
========================= */

fn exec(root: &mut MyValue, instructions: &Value) -> Result<(), InterpError> {
    let Value::List(list) = instructions else {
        // Top-level isn't a list: nothing to execute (file was a plain value).
        return Ok(());
    };
    let mut i = 0;
    while let Some(val) = list.get(&Value::Number(i)) {
        let Value::List(stmt) = val else {
            eprintln!("not a statement: {:?}", val);
            i += 1;
            continue;
        };
        let Some(lhs_pair) = stmt.get(&Value::Number(0)) else {
            eprintln!("statement missing lhs pair: {:?}", val);
            i += 1;
            continue;
        };
        let Some(rhs_pair) = stmt.get(&Value::Number(1)) else {
            eprintln!("statement missing rhs pair: {:?}", val);
            i += 1;
            continue;
        };
        if let Err(e) = do_set(root, lhs_pair, rhs_pair) {
            eprintln!("runtime error in {:?}: {:?}", val, e);
        }
        i += 1;
    }
    Ok(())
}

fn do_set(root: &mut MyValue, lhs_pair: &Value, rhs_pair: &Value) -> Result<(), InterpError> {
    let (lhs_expr, lhs_depth) = extract_pair_value(lhs_pair)?;
    let (rhs_expr, rhs_depth) = extract_pair_value(rhs_pair)?;

    let mut lhs = eval(root, lhs_expr)?;
    for _ in 0..lhs_depth {
        lhs = walk(root, iterate(&lhs))?.clone();
    }

    let mut rhs = eval(root, rhs_expr)?;
    for _ in 0..rhs_depth {
        rhs = walk(root, iterate(&rhs))?.clone();
    }

    assign(root, &lhs, rhs)
}

fn extract_pair_value(pair: &Value) -> Result<(&Value, i32), InterpError> {
    let Value::List(m) = pair else {
        return Err(InterpError::MalformedStatement);
    };
    let expr = m.get(&Value::Number(0)).ok_or(InterpError::MalformedStatement)?;
    let depth = match m.get(&Value::Number(1)) {
        Some(Value::Number(n)) => *n,
        None => 0,
        _ => return Err(InterpError::MalformedStatement),
    };
    Ok((expr, depth))
}

pub(crate) fn exec_mv(root: &mut MyValue, instructions: &MyValue) -> Result<(), InterpError> {
    let MyValue::Map(list) = instructions else {
        return Ok(());
    };
    let mut i = 0i32;
    while let Some(val) = list.get(&MyValue::Val(i)) {
        let MyValue::Map(stmt) = val else {
            eprintln!("not a statement: {:?}", val);
            i += 1;
            continue;
        };
        let Some(lhs_pair) = stmt.get(&MyValue::Val(0)) else {
            eprintln!("statement missing lhs pair: {:?}", val);
            i += 1;
            continue;
        };
        let Some(rhs_pair) = stmt.get(&MyValue::Val(1)) else {
            eprintln!("statement missing rhs pair: {:?}", val);
            i += 1;
            continue;
        };
        if let Err(e) = do_set_mv(root, lhs_pair, rhs_pair) {
            eprintln!("runtime error in {:?}: {:?}", val, e);
        }
        i += 1;
    }
    Ok(())
}

fn do_set_mv(root: &mut MyValue, lhs_pair: &MyValue, rhs_pair: &MyValue) -> Result<(), InterpError> {
    let (lhs_val, lhs_depth) = extract_pair_mv(lhs_pair)?;
    let (rhs_val, rhs_depth) = extract_pair_mv(rhs_pair)?;

    let mut lhs = lhs_val.clone();
    for _ in 0..lhs_depth {
        lhs = walk(root, iterate(&lhs))?.clone();
    }

    let mut rhs = rhs_val.clone();
    for _ in 0..rhs_depth {
        rhs = walk(root, iterate(&rhs))?.clone();
    }

    assign(root, &lhs, rhs)
}

fn extract_pair_mv(pair: &MyValue) -> Result<(&MyValue, i32), InterpError> {
    let MyValue::Map(m) = pair else {
        return Err(InterpError::MalformedStatement);
    };
    let val = m.get(&MyValue::Val(0)).ok_or(InterpError::MalformedStatement)?;
    let depth_val = m.get(&MyValue::Val(1));
    let depth = match depth_val {
        Some(v) => myvalue_to_num(v).ok_or(InterpError::MalformedStatement)?,
        None => 0,
    };
    Ok((val, depth))
}

/* =========================
   PRE-INITIALIZATION
========================= */

fn preinit_ops(root: &mut MyValue) {
    for &name in ops::registered_op_names() {
        ensure_path(root, &["ops", name, "trigger"], MyValue::Map(BTreeMap::new()));
    }
}

fn ensure_path(root: &mut MyValue, components: &[&str], val: MyValue) {
    let mut current = root;
    for c in components {
        if !matches!(current, MyValue::Map(_)) {
            *current = MyValue::Map(BTreeMap::new());
        }
        let MyValue::Map(m) = current else { unreachable!() };
        current = m.entry(str_to_myvalue(c)).or_insert_with(|| MyValue::Map(BTreeMap::new()));
    }
    *current = val;
}

/* =========================
   HELPERS
========================= */

fn build_path(components: &[&str]) -> MyValue {
    let mut m = BTreeMap::new();
    for (i, s) in components.iter().enumerate() {
        m.insert(MyValue::Val(i as i32), str_to_myvalue(s));
    }
    MyValue::Map(m)
}

pub(crate) fn str_to_myvalue(s: &str) -> MyValue {
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

pub(crate) fn lookup<'a>(v: &'a MyValue, key: &str) -> &'a MyValue {
    if let MyValue::Map(m) = v {
        if let Some(val) = m.get(&str_to_myvalue(key)) {
            return val;
        }
    }
    self_ref()
}

/* =========================
   PRETTY PRINTING
========================= */

fn print_value(v: &MyValue) {
    match v {
        MyValue::Self_ => print!("<self>"),
        MyValue::Val(n) => print!("{}", n),
        MyValue::Map(m) => {
            // Try unary number first; if it matches, render as decimal.
            if let Some(n) = myvalue_to_num(v) {
                print!("{}", n);
                return;
            }
            if m.is_empty() {
                // Unreachable in practice — empty map = 0 via myvalue_to_num —
                // but keep for safety.
                print!("{{}}");
                return;
            }
            if is_indexed(m) {
                if let Some(s) = try_as_string(m) {
                    print!("{:?}", s);
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

    let mut root = MyValue::Map(BTreeMap::new());
    preinit_ops(&mut root);
    exec(&mut root, &ast).unwrap();
    print_value(&root);
    println!();
}
