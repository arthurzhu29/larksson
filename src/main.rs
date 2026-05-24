use std::collections::BTreeMap;
use std::io::Read;
use std::ops::{BitAnd, BitOrAssign, Shl, Shr};

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

#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub enum Value {
    List(List),
    SelfSentinel,
}

type List = BTreeMap<Value, Value>;

impl Value {
    fn list_mut(&mut self) -> &mut List {
        match self {
            Self::List(l) => l,
            Self::SelfSentinel => panic!(),
        }
    }
    fn list(&self) -> &List {
        match self {
            Self::List(l) => l,
            Self::SelfSentinel => panic!(),
        }
    }
    fn get_list(&self) -> Option<&List> {
        match self {
            Self::List(l) => Some(l),
            Self::SelfSentinel => None,
        }
    }
    fn atomic(n: u32) -> Self {
        if n == 0 {
            return Value::SelfSentinel;
        }
        let mut current = Value::List(BTreeMap::new());
        for _ in 1 .. n {
            current = Value::List(BTreeMap::from([(Value::SelfSentinel, current)]));
        }
        current
    }
    fn unmarked_list(list: impl IntoIterator<Item = Value>) -> Self {
        let mut n = 0;
        Self::List(
            list.into_iter().map(|val| {
                let got = (Value::atomic(n), val);
                n += 1;
                got
            })
                .filter(|(_, val)| !matches!(val, Value::SelfSentinel))
                .collect()
        )
    }
    fn marked_list(len: usize, list: impl IntoIterator<Item = Value>) -> Self {
        Self::unmarked_list(std::iter::once(Value::atomic(len as u32)).chain(list))
    }
    fn _marked_list_len(&self) -> usize {
        self.list()[&Value::atomic(0)].try_atomic_to_u32().unwrap() as usize
    }
    fn get_marked_list_len(&self) -> Option<usize> {
        self.get_list()?.get(&Value::atomic(0))?.try_atomic_to_u32().map(|n| n as usize)
    }
    fn list_counted(list: impl IntoIterator<Item = Value>) -> Self {
        let mut n = 1;
        let mut res = Self::List(
            list.into_iter().map(|val| {
                let got = (Value::atomic(n), val);
                n += 1;
                got
            })
                .filter(|(_, val)| !matches!(val, Value::SelfSentinel))
                .collect()
        );
        res.list_mut().insert(Value::atomic(0), Value::atomic(n - 1));
        res
    }
}

impl Value {
    fn from_prim<T>(val: T) -> Self
    where
        T: Primitive,
        <T as Primitive>::Unsigned: Shr<u32, Output = <T as Primitive>::Unsigned>,
        <T as Primitive>::Unsigned: BitAnd<Output = <T as Primitive>::Unsigned>,
        <T as Primitive>::Unsigned: PartialEq,
    {
        Value::List(
            (0 .. T::BITS)
                .filter_map(
                    |i|
                    ((val.as_unsigned() >> i) & T::one() == T::one())
                        .then_some((Value::atomic(i + 1), Value::atomic(1)))
                )
                .chain([(Value::atomic(0), Value::atomic(T::BITS))])
                .collect()
        )
    }
    fn try_atomic_to_u32(&self) -> Option<u32> {
        if let Self::SelfSentinel = self {
            return Some(0);
        }
        let mut i = 1;
        let mut got = self.list();
        loop {
            if got.len() == 0 {
                return Some(i);
            }
            if got.len() > 1 {
                return None;
            }
            got = got.get(&Value::atomic(0))?.get_list()?;
            i += 1;
        }
    }
    fn try_to_prim<T>(&self) -> Option<T>
    where
        T: Primitive,
        <T as Primitive>::Unsigned: BitOrAssign<<T as Primitive>::Unsigned>,
        <T as Primitive>::Unsigned: Shl<u32, Output = <T as Primitive>::Unsigned>,
    {
        let map = self.get_list()?;
        if map.get(&Value::atomic(0))? != &Value::atomic(32) {
            return None;
        }
        let mut acc = T::zero();
        let mut count = 0;
        for i in 0 .. 32 {
            let Some(val) = map.get(&Value::atomic(i + 1)) else {
                continue;
            };
            if val != &Value::atomic(1) {
                return None;
            }
            acc |= T::one() << i;
            count += 1;
        }
        if map.len() != count + 1 {
            return None;
        }
        Some(T::from_unsigned(acc))
    }
}

trait Primitive: Copy {
    type Unsigned;
    const BITS: u32;
    fn as_unsigned(self) -> Self::Unsigned;
    fn from_unsigned(t: Self::Unsigned) -> Self;
    fn one() -> Self::Unsigned;
    fn zero() -> Self::Unsigned;
}
macro_rules! primitive {
    ($($($a:ty => $b: ty),+ $(,)?)?) => {
        $($(
            impl Primitive for $a {
                type Unsigned = $b;
                const BITS: u32 = Self::BITS;
                fn as_unsigned(self) -> Self::Unsigned { self as $b }
                fn from_unsigned(t: Self::Unsigned) -> Self { t as $a }
                fn one() -> Self::Unsigned { 1 }
                fn zero() -> Self::Unsigned { 0 }
            }
        )+)?
    };
}
primitive! {
    u8 => u8, u16 => u16, u32 => u32, u64 => u64, u128 => u128, usize => usize,
    i8 => u8, i16 => u16, i32 => u32, i64 => u64, i128 => u128, isize => isize,
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
    Value::unmarked_list(
        got.into_inner().map(|p| parse_value_or_set(p))
    )
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
            let (lhs_d, rhs_d) = match op.as_str() {
                "="  => (0, 0),
                "<-" => (0, 1),
                "->" => (1, 0),
                other => unreachable!("unknown assign op: {}", other),
            };
            let rhs = parse_value(inner.next().unwrap());

            Value::unmarked_list([
                Value::unmarked_list([lhs, Value::atomic(lhs_d)]),
                Value::unmarked_list([rhs, Value::atomic(rhs_d)]),
            ])
        }
        Rule::value => parse_value(pair),
        _ => unreachable!(),
    }
}

fn parse_value(pair: Pair<Rule>) -> Value {
    debug_assert_eq!(pair.as_rule(), Rule::value);
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::number => Value::from_prim(inner.as_str().parse::<i32>().expect("bad number")),
        Rule::atomic => Value::atomic(inner.into_inner().next().unwrap().as_str().parse().expect("bad number")),
        Rule::char_lit => Value::from_prim(parse_char_lit(inner.as_str())),
        Rule::string_lit => parse_string_lit(inner.as_str()),
        Rule::array => parse_indexed_unmarked(inner.into_inner()),
        Rule::dot_path => parse_indexed(inner.into_inner()),
        Rule::list => parse_list(inner.into_inner()),
        Rule::string => Value::list_counted(inner.as_str().chars().map(|c| Value::from_prim(c as u32))),
        Rule::self_lit => Value::SelfSentinel,
        r => unreachable!("unexpected rule in parse_value: {:?}", r),
    }
}

fn parse_indexed_unmarked(pairs: Pairs<Rule>) -> Value {
    Value::unmarked_list(
        pairs.map(|p| {
            debug_assert_eq!(p.as_rule(), Rule::value);
            parse_value(p)
        })
    )
}
fn parse_indexed(pairs: Pairs<Rule>) -> Value {
    Value::list_counted(
        pairs.map(|p| {
            debug_assert_eq!(p.as_rule(), Rule::value);
            parse_value(p)
        })
    )
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
    let mut chars = inner.chars();
    Value::list_counted(
        std::iter::from_fn(|| decode_escaped_char(&mut chars).map(Value::from_prim))
    )
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


#[derive(Debug)]
pub enum InterpError {
    MalformedStatement,
}


/* =========================
   PATH MACHINERY
========================= */

fn iterate_marked(x: &Value) -> impl Iterator<Item = &Value> {
    let m = x.get_list();
    (0 .. x.get_marked_list_len().unwrap_or(0))
        .map(move |i| m.and_then(|list| list.get(&Value::atomic(i as u32 + 1))).unwrap_or(SELF_REF))
}

const SELF_REF: &Value = &Value::SelfSentinel;

fn walk<'a, 'b>(root: &'a Value, path: impl Iterator<Item = &'b Value>) -> &'a Value {
    let mut current = root;
    for key in path {
        current = match current {
            Value::List(m) => {
                m.get(key).unwrap_or(SELF_REF)
            },
            Value::SelfSentinel => break,
        }
    }
    current
}

/* =========================
   ASSIGNMENT
========================= */

fn assign(root: &mut Value, path: &Value, val: Value) -> Result<(), InterpError> {
    let components: Vec<&Value> = iterate_marked(path).collect();

    if components.is_empty() {
        panic!("cannot replace root: writes must go through a namespace");
    }

    check_write_path(&components);
    check_write_value(&components, &val);

    // Writing self removes the keyed entry.
    if matches!(val, Value::SelfSentinel) {
        remove_at(root, &components);
        return Ok(());
    }

    // Walk to the target slot, creating intermediate maps as needed.
    let mut current = &mut *root;
    for key in &components {
        if !matches!(current, Value::List(_)) {
            *current = Value::List(BTreeMap::new());
        }
        current = current.list_mut().entry((*key).clone()).or_insert_with(|| Value::List(BTreeMap::new()));
    }
    *current = val;

    let written = walk(root, iterate_marked(path)).clone();
    maybe_fire(root, &components, &written)
}

fn remove_at(root: &mut Value, components: &[&Value]) {
    if components.is_empty() {
        return;
    }
    let mut current = &mut *root;
    for key in &components[..components.len() - 1] {
        match current {
            Value::List(m) => {
                if let Some(next) = m.get_mut(*key) {
                    current = next;
                } else {
                    return;
                }
            }
            _ => return,
        }
    }
    if let Value::List(m) = current {
        m.remove(components[components.len() - 1]);
    }
}

/* =========================
   NAMESPACE AND TRIGGER ENFORCEMENT
========================= */

fn check_write_path(components: &[&Value]) {
    let first = value_as_string(components[0])
        .unwrap_or_else(|| panic!("first path component must be a string namespace"));
    match first.as_str() {
        "ops" => {
            if components.len() < 2 {
                panic!("cannot write to .ops. directly");
            }
            let opname = value_as_string(components[1])
                .unwrap_or_else(|| panic!("ops name must be a string"));
            if ops::op_registry(&opname).is_none() {
                panic!("unknown op: '{}'", opname);
            }
        }
        "var" => {}
        other => panic!("forbidden namespace: '{}' (allowed: ops, var)", other),
    }
}

fn check_write_value(components: &[&Value], val: &Value) {
    if value_as_string(components[0]).as_deref() != Some("ops") {
        return;
    }

    // Removing .ops.<op>. or .ops.<op>.trigger. is illegal (kills the trigger slot).
    if matches!(val, Value::SelfSentinel) {
        if components.len() == 2 {
            panic!("cannot delete .ops.<op>. (would remove trigger field)");
        }
        if components.len() == 3
            && value_as_string(components[2]).as_deref() == Some("trigger")
        {
            panic!("cannot remove trigger field");
        }
        return;
    }

    // Writing to .ops.<op>.trigger. directly: must be zero.
    if components.len() == 3
        && value_as_string(components[2]).as_deref() == Some("trigger")
    {
        validate_trigger_value(val);
        return;
    }

    // Writing the whole op record: if it contains a trigger key, validate that.
    if components.len() == 2 {
        if let Value::List(m) = val {
            if let Some(trigger_val) = m.get(&str_to_value("trigger")) {
                validate_trigger_value(trigger_val);
            }
        }
    }
}

fn validate_trigger_value(val: &Value) {
    // Accept either representation of zero: Val(0) or Map(empty).
    match val {
        Value::List(m) if m.is_empty() => {}
        _ => panic!("trigger may only be set to 0 or {{}}, got {:?}", val),
    }
}

/* =========================
   TRIGGER FIRING
========================= */

fn maybe_fire(
    root: &mut Value,
    components: &[&Value],
    written: &Value,
) -> Result<(), InterpError> {
    if components.len() < 2 { return Ok(()); }
    if value_as_string(components[0]).as_deref() != Some("ops") { return Ok(()); }
    let Some(opname) = value_as_string(components[1]) else { return Ok(()); };

    if components.len() >= 3
        && value_as_string(components[2]).as_deref() == Some("trigger")
    {
        return fire_op(root, &opname);
    }

    if components.len() == 2 {
        if let Value::List(m) = written {
            if m.contains_key(&str_to_value("trigger")) {
                return fire_op(root, &opname);
            }
        }
    }

    Ok(())
}

fn fire_op(root: &mut Value, opname: &str) -> Result<(), InterpError> {
    let op_fn = ops::op_registry(opname).unwrap();
    let args_path = build_path(&["ops", opname, "args"]);
    let args = walk(root, iterate_marked(&args_path)).clone();
    let result = op_fn(root, &args)?;
    let return_path = build_path(&["ops", opname, "return"]);
    assign(root, &return_path, result)
}

/* =========================
   STATEMENT EXECUTION
========================= */

fn exec(root: &mut Value, instructions: &Value) -> Result<(), InterpError> {
    let Value::List(list) = instructions else {
        // Top-level isn't a list: nothing to execute (file was a plain value).
        return Ok(());
    };
    let mut i = 0;
    while let Some(val) = list.get(&Value::atomic(i)) {
        let Value::List(stmt) = val else {
            eprintln!("not a statement: {:?}", val);
            i += 1;
            continue;
        };
        let Some(lhs_pair) = stmt.get(&Value::atomic(0)) else {
            eprintln!("statement missing lhs pair: {:?}", val);
            i += 1;
            continue;
        };
        let Some(rhs_pair) = stmt.get(&Value::atomic(1)) else {
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

fn do_set(root: &mut Value, lhs_pair: &Value, rhs_pair: &Value) -> Result<(), InterpError> {
    let (mut lhs, lhs_depth) = extract_pair_value(lhs_pair)?;
    let (mut rhs, rhs_depth) = extract_pair_value(rhs_pair)?;

    for _ in 0..lhs_depth {
        lhs = walk(root, iterate_marked(&lhs));
    }

    for _ in 0..rhs_depth {
        rhs = walk(root, iterate_marked(&rhs));
    }

    let (lhs, rhs) = (lhs.clone(), rhs.clone());

    assign(root, &lhs, rhs)
}

fn extract_pair_value(pair: &Value) -> Result<(&Value, i32), InterpError> {
    let m = pair.get_list().ok_or(InterpError::MalformedStatement)?;
    let expr = m.get(&Value::atomic(0)).unwrap_or(SELF_REF);
    let depth = m.get(&Value::atomic(1)).unwrap_or(SELF_REF);
    let depth = depth.try_atomic_to_u32().unwrap_or_default();
    Ok((expr, depth as i32))
}


/* =========================
   PRE-INITIALIZATION
========================= */

fn preinit_ops(root: &mut Value) {
    for &name in ops::registered_op_names() {
        ensure_path(root, &["ops", name, "trigger"], Value::List(BTreeMap::new()));
    }
}

fn ensure_path(root: &mut Value, components: &[&str], val: Value) {
    let mut current = root;
    for c in components {
        if !matches!(current, Value::List(_)) {
            *current = Value::List(BTreeMap::new());
        }
        current = current.list_mut().entry(str_to_value(c)).or_insert_with(|| Value::List(BTreeMap::new()));
    }
    *current = val;
}

/* =========================
   HELPERS
========================= */

fn build_path(components: &[&str]) -> Value {
    Value::marked_list(
        components.len(),
        components.iter().map(|s| str_to_value(*s))
    )
}

pub(crate) fn str_to_value(s: &str) -> Value {
    Value::list_counted(
        s.chars().map(|c| Value::from_prim(c as u32))
    )
}

fn value_as_string(v: &Value) -> Option<String> {
    let m = v.get_list()?;
    let len = m.get(&Value::atomic(0))?.try_atomic_to_u32()?;
    if m.len() != len as usize + 1 {
        return None;
    }
    (1 ..= len)
        .map(|i| char::from_u32(
            m.get(&Value::atomic(i))?.try_to_prim()?
        ))
        .collect()
}

pub(crate) fn lookup<'a>(v: &'a Value, key: &str) -> &'a Value {
    v
        .get_list()
        .and_then(|m| m.get(&str_to_value(key)))
        .unwrap_or(SELF_REF)
}

/* =========================
   PRETTY PRINTING
========================= */

fn print_value(v: &Value) {
    match v {
        Value::SelfSentinel => print!("<self>"),
        Value::List(m) => {
            // Try unary number first; if it matches, render as decimal.
            if let Some(n) = v.try_atomic_to_u32() {
                print!("*{}", n);
                return;
            }
            if let Some(n) = v.try_to_prim::<u32>() {
                print!("{}", n);
                return;
            }
            if m.is_empty() {
                // Unreachable in practice — empty map = 0 via Value_to_num —
                // but keep for safety.
                print!("{{}}");
                return;
            }
            if let Some(len) = is_indexed_marked(m) {
                if let Some(s) = value_as_string(v) {
                    if s.chars().all(|c| c.is_alphanumeric()) {
                        print!("{}", s);
                    } else {
                        print!("\"{}\"", s);
                    }
                    return;
                }
                for i in 1 ..= len as u32 {
                    print!(".");
                    print_value(&m.get(&Value::atomic(i)).unwrap_or(SELF_REF));
                }
                print!(".");
                return;
            }
            if is_indexed_unmarked(m) {
                print!("[");
                for i in 0 .. m.len() as u32 {
                    if i > 0 {
                        print!(", ");
                    }
                    print_value(&m[&Value::atomic(i)]);
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

fn is_indexed_unmarked(m: &BTreeMap<Value, Value>) -> bool {
    (0 .. (m.len() as u32)).all(|i| m.contains_key(&Value::atomic(i)))
}
fn is_indexed_marked(m: &List) -> Option<usize> {
    let len = m.get(&Value::atomic(0))?.try_atomic_to_u32()?;
    m
        .keys()
        .all(|key| key.try_atomic_to_u32().is_some_and(|i| i <= len))
        .then_some(len as usize)
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

    let mut root = Value::List(BTreeMap::new());
    preinit_ops(&mut root);
    exec(&mut root, &ast).unwrap();
    print_value(&root);
    println!();
}
