use std::collections::BTreeMap;
use std::io::Read;
use std::ops::{BitAnd, BitOrAssign, Shl, Shr};

use pest::Parser;
use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;

mod ops;
mod main_copy;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct MyParser;

/* =========================
   AST
========================= */

#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct L(BTreeMap<V, V>);

impl<const N: usize> From<[(V, V); N]> for L {
    fn from(value: [(V, V); N]) -> Self {
        L(
            value
                .into_iter()
                .filter(|(_, val)| val.0.is_some())
                .collect()
        )
    }
}
impl L {
    fn new() -> Self {
        L(BTreeMap::new())
    }
    fn insert(&mut self, key: V, val: V) {
        if val.0.is_some() {
            self.0.insert(key, val);
        }
    }
    fn get(&self, key: &V) -> Option<&V> {
        self.0.get(key)
    }
    fn get_mut(&mut self, key: &V) -> Option<&mut V> {
        self.0.get_mut(key)
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn entry(&mut self, key: V) -> std::collections::btree_map::Entry<V, V> {
        self.0.entry(key)
    }
    fn remove(&mut self, key: &V) -> Option<V> {
        self.0.remove(key)
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn contains_key(&self, key: &V) -> bool {
        self.0.contains_key(key)
    }
    fn keys(&self) -> std::collections::btree_map::Keys<V, V> {
        self.0.keys()
    }
    fn iter(&self) -> std::collections::btree_map::Iter<V, V> {
        self.0.iter()
    }
}
impl FromIterator<(V, V)> for L {
    fn from_iter<T: IntoIterator<Item = (V, V)>>(iter: T) -> Self {
        L(BTreeMap::from_iter(iter))
    }
}
impl std::ops::Index<&V> for L {
    type Output = V;
    fn index(&self, index: &V) -> &Self::Output {
        self.0.get(index).unwrap_or(&V(None))
    }
}
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct V(Option<L>);

impl From<()> for V {
    fn from((): ()) -> Self {
        Self(None)
    }
}
impl From<L> for V {
    fn from(value: L) -> Self {
        Self(Some(value))
    }
}

fn increment(n: &mut usize) -> usize {
    let got = *n;
    *n += 1;
    got
}

impl V {
    fn atomic(n: u32) -> Self {
        if n == 0 {
            return ().into();
        }
        let mut current = L::new().into();
        for _ in 1 .. n {
            current = L::from([(V(None), current)]).into();
        }
        current
    }
    fn system_list(list: impl IntoIterator<Item = V>) -> Self {
        list.into_iter()
            .enumerate()
            .map(|(i, val)| (V::atomic(i as u32), val))
            .collect::<L>()
            .into()
    }
    fn _marked_list(len: usize, list: impl IntoIterator<Item = V>) -> Self {
        list.into_iter()
            .enumerate()
            .map(|(i, val)| (V::from_prim(i), val))
            .chain([(().into(), V::from_prim(len))])
            .collect::<L>()
            .into()
    }
    fn marked_list_counted(list: impl IntoIterator<Item = V>) -> Self {
        let mut n = 0usize;
        let mut m = list
            .into_iter()
            .map(|val| (V::from_prim(increment(&mut n)), val))
            .collect::<L>();
        m.insert(().into(), V::from_prim(n));
        m.into()
    }
    fn _marked_list_len(&self) -> usize {
        self.0.as_ref().unwrap()[&V::atomic(0)].try_atomic_to_u32().unwrap() as usize
    }
    fn get_marked_list_len(&self) -> Option<usize> {
        self.0.as_ref()?.get(&V::atomic(0))?.try_atomic_to_u32().map(|n| n as usize)
    }
    // empty -> <>
    // item -> { <>: item }
    // .a.b. -> { <>: a, {}: { <>: b }}
    fn linked_list<T>(list: T) -> Self
    where
        T: IntoIterator<Item = V>,
        T::IntoIter: DoubleEndedIterator,
    {
        let mut got = ().into();
        for item in list.into_iter().rev() {
            got = L::from([(().into(), item), (L::new().into(), got)]).into()
        }
        got
    }
}

impl V {
    fn from_prim<T>(val: T) -> Self
    where
        T: Primitive,
        <T as Primitive>::Unsigned: Shr<u32, Output = <T as Primitive>::Unsigned>,
        <T as Primitive>::Unsigned: BitAnd<Output = <T as Primitive>::Unsigned>,
        <T as Primitive>::Unsigned: PartialEq,
    {
        V(Some(
            (0 .. T::BITS)
                .filter_map(
                    |i|
                    ((val.as_unsigned() >> i) & T::one() == T::one())
                        .then_some((V::atomic(i + 1), V::atomic(1)))
                )
                .chain([(V::atomic(0), V::atomic(T::BITS))])
                .collect()
        ))
    }
    fn try_atomic_to_u32(&self) -> Option<u32> {
        if let Self(None) = self {
            return Some(0);
        }
        let mut i = 1;
        let mut got = self.0.as_ref()?;
        loop {
            if got.len() == 0 {
                return Some(i);
            }
            if got.len() > 1 {
                return None;
            }
            got = got.get(&V::atomic(0))?.0.as_ref()?;
            i += 1;
        }
    }
    fn try_to_prim<T>(&self) -> Option<T>
    where
        T: Primitive,
        <T as Primitive>::Unsigned: BitOrAssign<<T as Primitive>::Unsigned>,
        <T as Primitive>::Unsigned: Shl<u32, Output = <T as Primitive>::Unsigned>,
    {
        let map = self.0.as_ref()?;
        if map.get(&V::atomic(0))? != &V::atomic(32) {
            return None;
        }
        let mut acc = T::zero();
        let mut count = 0;
        for i in 0 .. T::BITS {
            let Some(val) = map.get(&V::atomic(i + 1)) else {
                continue;
            };
            if val != &V::atomic(1) {
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

fn build_ast(mut pairs: Pairs<Rule>) -> V {
    let got = pairs.next().unwrap();
    if let Rule::value = got.as_rule() {
        return parse_value(got);
    }
    V::system_list(
        got.into_inner().map(|p| parse_value_or_set(p))
    )
}

/// Statement shape: [[lhs, lhs_depth], [rhs, rhs_depth]].
///   `a = b`  -> [[a, 0], [b, 0]]
///   `a <- b` -> [[a, 0], [b, 1]]
///   `a -> b` -> [[a, 1], [b, 0]]
fn parse_value_or_set(pair: Pair<Rule>) -> V {
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

            V::system_list([
                V::system_list([lhs, V::atomic(lhs_d)]),
                V::system_list([rhs, V::atomic(rhs_d)]),
            ])
        }
        Rule::value => parse_value(pair),
        _ => unreachable!(),
    }
}

fn parse_value(pair: Pair<Rule>) -> V {
    debug_assert_eq!(pair.as_rule(), Rule::value);
    let inner = pair.into_inner().next().unwrap();
    let s = inner.as_str();
    match inner.as_rule() {
        Rule::number => V::from_prim(s.parse::<i32>().expect("bad number")),
        Rule::atomic => V::atomic(
            inner
                .into_inner().next().unwrap()
                .as_str().parse().expect("bad number")
        ),
        Rule::char_lit => V::from_prim({
            let inner = &s[1..s.len() - 1];
            decode_escaped_char(&mut inner.chars()).expect("empty char literal")
        }),
        Rule::string_lit => {
            let inner = &s[1 .. s.len() - 1];
            let mut chars = inner.chars();
            V::marked_list_counted({
                std::iter::from_fn(|| decode_escaped_char(&mut chars).map(V::from_prim))
            })
        },
        Rule::array => V::marked_list_counted(
            inner
                .into_inner()
                .map(|pair| {
                    debug_assert_eq!(pair.as_rule(), Rule::value);
                    parse_value(pair)
                })
        ),
        Rule::dot_path => V::linked_list(
            inner
                .into_inner()
                .map(|pair| {
                    debug_assert_eq!(pair.as_rule(), Rule::value);
                    parse_value(pair)
                })
        ),
        Rule::list => inner
            .into_inner()
            .filter_map(
                |p|
                (p.as_rule() == Rule::list_item)
                    .then_some({
                        let mut it = p.into_inner();
                        (parse_value(it.next().unwrap()), parse_value(it.next().unwrap()))
                    })
            )
            .collect::<L>()
            .into(),
        Rule::string => V::marked_list_counted(
            inner.as_str().chars().map(|c| V::from_prim(c as u32))
        ),
        Rule::self_lit => ().into(),
        r => unreachable!("unexpected rule in parse_value: {:?}", r),
    }
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

fn iterate_marked(x: &V) -> impl Iterator<Item = &V> {
    let m = x.0.as_ref();
    (0 .. x.get_marked_list_len().unwrap_or(0))
        .map(move |i| m.and_then(|list| list.get(&V::atomic(i as u32 + 1))).unwrap_or(SELF_REF))
}

const SELF_REF: &V = &V(None);

fn walk<'a, 'b>(root: &'a V, path: impl Iterator<Item = &'b V>) -> &'a V {
    let mut current = root;
    for key in path {
        if let Some(m) = &current.0 {
            current = m.get(key).unwrap_or(SELF_REF);
        } else {
            break;
        }
    }
    current
}

/* =========================
   ASSIGNMENT
========================= */

fn assign(root: &mut V, path: &V, val: V) -> Result<(), InterpError> {
    let components: Vec<&V> = iterate_marked(path).collect();

    if components.is_empty() {
        panic!("cannot replace root: writes must go through a namespace");
    }

    check_write_path(&components);
    check_write_value(&components, &val);

    // Writing self removes the keyed entry.
    if matches!(val, V(None)) {
        remove_at(root, &components);
        return Ok(());
    }

    // Walk to the target slot, creating intermediate maps as needed.
    let mut current = &mut *root;
    for key in &components {
        if current.0.is_none() {
            current.0.insert(L::new());
        }
        current = current.0.as_mut().unwrap().entry((*key).clone()).or_insert_with(|| L::new().into());
    }
    *current = val;

    let written = walk(root, iterate_marked(path)).clone();
    maybe_fire(root, &components, &written)
}

fn remove_at(root: &mut V, components: &[&V]) {
    if components.is_empty() {
        return;
    }
    let mut current = &mut *root;
    for key in &components[..components.len() - 1] {
        if let Some(m) = &mut current.0
            && let Some(next) = m.get_mut(*key)
        {
            current = next;
        } else {
            return;
        }
    }
    if let Some(m) = &mut current.0 {
        m.remove(components[components.len() - 1]);
    }
}

/* =========================
   NAMESPACE AND TRIGGER ENFORCEMENT
========================= */

fn check_write_path(components: &[&V]) {
    let first = value_as_string(components[0])
        .unwrap_or_else(|| panic!("first path component must be a string namespace"));
    match first.as_str() {
        "ops" => {
            if components.len() < 2 {
                panic!("cannot write to .ops. directly");
            }
            let opname = value_as_string(components[1])
                .unwrap_or_else(|| panic!("ops name must be a string"));
            if crate::ops::op_registry(&opname).is_none() {
                panic!("unknown op: '{}'", opname);
            }
        }
        "var" => {}
        other => panic!("forbidden namespace: '{}' (allowed: ops, var)", other),
    }
}

fn check_write_value(components: &[&V], val: &V) {
    if value_as_string(components[0]).as_deref() != Some("ops") {
        return;
    }

    // Removing .ops.<op>. or .ops.<op>.trigger. is illegal (kills the trigger slot).
    if matches!(val, V(None)) {
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
        if let Some(m) = &val.0 {
            if let Some(trigger_val) = m.get(&str_to_value("trigger")) {
                validate_trigger_value(trigger_val);
            }
        }
    }
}

fn validate_trigger_value(val: &V) {
    // Accept either representation of zero: Val(0) or L(empty).
    if !val.0.as_ref().is_some_and(|m| m.is_empty()) {
        panic!("trigger may only be set to 0 or {{}}, got {:?}", val);
    }
}

/* =========================
   TRIGGER FIRING
========================= */

fn maybe_fire(
    root: &mut V,
    components: &[&V],
    written: &V,
) -> Result<(), InterpError> {
    if components.len() < 2 { return Ok(()); }
    if value_as_string(components[0]).as_deref() != Some("ops") { return Ok(()); }
    let Some(opname) = value_as_string(components[1]) else { return Ok(()); };

    if (
            components.len() >= 3
                && value_as_string(components[2]).as_deref() == Some("trigger")
        )
            || (
                components.len() == 2
                    && written.0.as_ref().is_some_and(|m| m.contains_key(&str_to_value("trigger")))
            )
    {
        return fire_op(root, &opname);
    }

    Ok(())
}

fn fire_op(root: &mut V, opname: &str) -> Result<(), InterpError> {
    let op_fn = crate::ops::op_registry(opname).unwrap();
    let args_path = build_path(&["ops", opname, "args"]);
    let args = walk(root, iterate_marked(&args_path)).clone();
    let result = op_fn(root, &args)?;
    let return_path = build_path(&["ops", opname, "return"]);
    assign(root, &return_path, result)
}

/* =========================
   STATEMENT EXECUTION
========================= */

fn exec(root: &mut V, instructions: &V) -> Result<(), InterpError> {
    let Some(list) = &instructions.0 else {
        // Top-level isn't a list: nothing to execute (file was a plain value).
        return Ok(());
    };
    let mut i = 0;
    while let Some(val) = list.get(&V::atomic(i)) {
        i += 1;
        let Some(stmt) = &val.0 else {
            eprintln!("not a statement: {:?}", val);
            continue;
        };
        let Some(lhs_pair) = stmt.get(&V::atomic(0)) else {
            eprintln!("statement missing lhs pair: {:?}", val);
            continue;
        };
        let Some(rhs_pair) = stmt.get(&V::atomic(1)) else {
            eprintln!("statement missing rhs pair: {:?}", val);
            continue;
        };
        if let Err(e) = do_set(root, lhs_pair, rhs_pair) {
            eprintln!("runtime error in {:?}: {:?}", val, e);
        }
    }
    Ok(())
}

fn do_set(root: &mut V, lhs_pair: &V, rhs_pair: &V) -> Result<(), InterpError> {
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

fn extract_pair_value(pair: &V) -> Result<(&V, i32), InterpError> {
    let m = pair.0.as_ref().ok_or(InterpError::MalformedStatement)?;
    let expr = m.get(&V::atomic(0)).unwrap_or(SELF_REF);
    let depth = m.get(&V::atomic(1)).unwrap_or(SELF_REF);
    let depth = depth.try_atomic_to_u32().unwrap_or_default();
    Ok((expr, depth as i32))
}


/* =========================
   PRE-INITIALIZATION
========================= */

fn preinit_ops(root: &mut V) {
    for &name in crate::ops::registered_op_names() {
        ensure_path(root, &["ops", name, "trigger"], L::new().into());
    }
}

fn ensure_path(root: &mut V, components: &[&str], val: V) {
    let mut current = root;
    for c in components {
        if current.0.is_none() {
            current.0.insert(L::new());
        }
        current = current[()].entry(str_to_value(c)).or_insert(L::new().into());
    }
    *current = val;
}

impl std::ops::Index<()> for V {
    type Output = L;
    fn index(&self, (): ()) -> &Self::Output {
        self.0.as_ref().unwrap()
    }
}
impl std::ops::IndexMut<()> for V {
    fn index_mut(&mut self, (): ()) -> &mut Self::Output {
        self.0.as_mut().unwrap()
    }
}

/* =========================
   HELPERS
========================= */

fn build_path(components: &[&str]) -> V {
    V::linked_list(
        components.iter().map(|s| str_to_value(*s))
    )
}

pub(crate) fn str_to_value(s: &str) -> V {
    V::marked_list_counted(
        s.chars().map(|c| V::from_prim(c as u32))
    )
}

fn value_as_string(v: &V) -> Option<String> {
    let m = v.0.as_ref()?;
    let len = m.get(&V::atomic(0))?.try_atomic_to_u32()?;
    if m.len() != len as usize + 1 {
        return None;
    }
    (1 ..= len)
        .map(|i| char::from_u32(
            m.get(&V::atomic(i))?.try_to_prim()?
        ))
        .collect()
}

pub(crate) fn lookup<'a>(v: &'a V, key: &str) -> &'a V {
    v
        .0
        .as_ref()
        .and_then(|m| m.get(&str_to_value(key)))
        .unwrap_or(SELF_REF)
}

/* =========================
   PRETTY PRINTING
========================= */

fn print_value(v: &V) {
    let Some(m) = &v.0 else {
        print!("<self>");
        return;
    };
    // Try unary number first; if it matches, render as decimal.
    if let Some(n) = v.try_atomic_to_u32() {
        print!("*{}", n);
    } else if let Some(n) = v.try_to_prim::<u32>() {
        print!("{}", n);
    } else if m.is_empty() {
        // Unreachable in practice — empty map = 0 via Value_to_num —
        // but keep for safety.
        print!("{{}}");
    } else if let Some(len) = is_indexed_marked(m) {
        if let Some(s) = value_as_string(v) {
            if s.chars().all(|c| c.is_alphanumeric()) {
                print!("{}", s);
            } else {
                print!("\"{}\"", s);
            }
        } else {
            for i in 1 ..= len as u32 {
                print!(".");
                print_value(&m.get(&V::atomic(i)).unwrap_or(SELF_REF));
            }
            print!(".");
        }
    } else if is_indexed_unmarked(m) {
        print!("[");
        for i in 0 .. m.len() as u32 {
            if i > 0 {
                print!(", ");
            }
            print_value(&m[&V::atomic(i)]);
        }
        print!("]");
    } else {
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

fn is_indexed_unmarked(m: &L) -> bool {
    (0 .. (m.len() as u32)).all(|i| m.contains_key(&V::atomic(i)))
}
fn is_indexed_marked(m: &L) -> Option<usize> {
    let len = m.get(&V::atomic(0))?.try_atomic_to_u32()?;
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

    let mut root = L::new().into();
    preinit_ops(&mut root);
    exec(&mut root, &ast).unwrap();
    print_value(&root);
    println!();
}
