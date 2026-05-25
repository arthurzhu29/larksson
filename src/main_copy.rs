#![allow(unused)]

use std::collections::BTreeMap;
use std::env::args;
use std::io::Read;
use std::ops::{BitAnd, BitOr, BitOrAssign, Shl, Shr};

use pest::Parser;
use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;

use crate::ops::*;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct MyParser;

/* =========================
*/

mod l;
use l::L;

#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct V(Option<L>);

impl From<()> for V {
    fn from((): ()) -> Self {
        Self(None)
    }
}
impl From<Atomic> for V {
    fn from(Atomic(n): Atomic) -> Self {
        if n == 0 {
            return ().into();
        }
        let mut current = L::new().into();
        for _ in 1 .. n {
            current = L::from([(V(None), current)]).into();
        }
        current
    }
}

fn increment(n: &mut usize) -> usize {
    let got = *n;
    *n += 1;
    got
}


impl V {
    fn _marked_list_len(&self) -> usize {
        self.0.as_ref().unwrap().get_self().unwrap().try_to_prim().unwrap()
    }
    fn get_marked_list_len(&self) -> Option<usize> {
        self.0.as_ref()?.get_self()?.try_to_prim()
    }
}

fn bit_set<T: Primitive>(val: T, idx: u32) -> bool
where
    T: Primitive,
    <T as Primitive>::Unsigned: Shr<u32, Output = <T as Primitive>::Unsigned>,
    <T as Primitive>::Unsigned: BitAnd<Output = <T as Primitive>::Unsigned>,
    <T as Primitive>::Unsigned: PartialEq,
{
    (val.as_unsigned() >> idx) & T::one() == T::one()
}

impl V {
    fn from_prim<T>(val: T) -> Self
    where
        T: Primitive,
        <T as Primitive>::Unsigned: Shr<u32, Output = <T as Primitive>::Unsigned>,
        <T as Primitive>::Unsigned: BitAnd<Output = <T as Primitive>::Unsigned>,
        <T as Primitive>::Unsigned: PartialEq,
    {
        V::list::<SysList>(
            (0 .. T::BITS)
                .map(|i| Atomic(bit_set(val, i) as u32).into())
        )
    }
    fn try_to_prim<T>(&self) -> Option<T>
    where
        T: Primitive,
        <T as Primitive>::Unsigned: BitOr<Output = <T as Primitive>::Unsigned>,
        <T as Primitive>::Unsigned: Shl<u32, Output = <T as Primitive>::Unsigned>,
    {
        let map = self.0.as_ref()?;
        let val = {
            map
                .iter()
                .try_fold(
                    T::zero(),
                    |acc, (key, val)| {
                        let n = key.try_into_t::<Atomic>().ok()?.0;
                        Some(acc | T::one() << n)
                    },
                )?
        };
        T::from_unsigned(val).some()
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
    i8 => u8, i16 => u16, i32 => u32, i64 => u64, i128 => u128, isize => usize,
}


/* =========================
   AST BUILDER
========================= */

fn build_ast(mut pairs: Pairs<Rule>) -> V {
    let got = pairs.next().unwrap();
    if let Rule::value = got.as_rule() {
        return parse_value(got);
    }
    V::list::<SysList>(
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

            V::list::<SysList>([
                V::list::<SysList>([lhs, Atomic(lhs_d).into()]),
                V::list::<SysList>([rhs, Atomic(rhs_d).into()]),
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
        Rule::atomic => Atomic(
            inner
                .into_inner().next().unwrap()
                .as_str().parse().expect("bad number")
        )
            .into(),
        Rule::char_lit => V::from_prim({
            let inner = &s[1..s.len() - 1];
            decode_escaped_char(&mut inner.chars()).expect("empty char literal")
        }),
        Rule::string_lit => {
            let inner = &s[1 .. s.len() - 1];
            let mut chars = inner.chars();
            V::list::<MarkedList>({
                std::iter::from_fn(|| decode_escaped_char(&mut chars).map(V::from_prim))
            })
        },
        Rule::array => V::list::<MarkedList>(
            inner
                .into_inner()
                .map(|pair| {
                    debug_assert_eq!(pair.as_rule(), Rule::value);
                    parse_value(pair)
                })
        ),
        Rule::sys_list => V::list::<SysList>(
            inner
                .into_inner()
                .map(|pair| {
                    debug_assert_eq!(pair.as_rule(), Rule::value);
                    parse_value(pair)
                })
        ),
        Rule::dot_path => V::list::<LinkedList>(
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
        Rule::string => V::list::<MarkedList>(
            inner.as_str().chars().map(|c| V::from_prim(c as u32))
        ),
        Rule::self_lit => ().into(),
        r @ (
            Rule::EOI | Rule::WHITESPACE | Rule::file | Rule::lines | Rule::assign_op
                | Rule::list_item | Rule::set_statement | Rule::value
        ) => unreachable!("unexpected rule in parse_value: {:?}", r),
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

mod list;
use list::{MarkedList, SysList, LinkedList, GetList};

/* =========================
   PATH MACHINERY
========================= */


impl V {
    fn iterate_where_self_is_sys_list<const MUST_BE_MAP: bool>(&self) -> impl Iterator<Item = &V> {
        let m = self.0.as_ref();
        if MUST_BE_MAP && m.is_none() {
            panic!("sys list not map");
        }
        (0 ..).map_while(move |i| m.and_then(|map| map.get_atomic(i)))
    }
}

const SELF_REF: &V = &V(None);

fn walk<'a, 'b>(root: &'a V, path: impl IntoIterator<Item = &'b V>) -> &'a V {
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

fn execute_an_assignment_which_might_delete_instead_and_might_fire(root: &mut V, path: &V, mut val: V) -> Result<(), InterpError> {
    let components: Vec<&V> = path.iter::<MarkedList>().collect();

    panic_if_write_path_is_not_dot_sys_dot_x_dot_or_dot_var_dot_whatever(&components);

    // Writing self removes the keyed entry.
    if val.0.is_none() {
        remove_the_node_at_this_position_by_indexing_into_parent(root, &components);
        return Ok(());
    }

    remove_nested_self_values(&mut val);
    // Walk to the target slot, creating intermediate maps as needed.
    doing_the_actual_assignment_no_more_checks(
        root,
        components.iter().map(|&k| k.clone()),
        val,
    );

    let written = walk(root, path.iter::<MarkedList>()).clone();
    maybe_fire(root, &components, &written)
}

fn remove_nested_self_values(val: &mut V) {
    let Some(m) = &mut val.0 else {
        return;
    };
    m.retain(|_, v| v.0.is_some());
    for v in m.values_mut() {
        remove_nested_self_values(v);
    }
}

struct Copied<T>(T);
impl<'a, I, T: 'a> IntoIterator for Copied<I>
where
    I: IntoIterator<Item = &'a T>,
    T: Copy,
{
    type Item = T;
    type IntoIter = std::iter::Copied<I::IntoIter>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter().copied()
    }
}

fn remove_the_node_at_this_position_by_indexing_into_parent(root: &mut V, components: &[&V]) {
    if components.is_empty() {
        return;
    }
    if let Some(V(Some(m))) = try_index_path_mut(root, Copied(&components[.. components.len() - 1])) {
        m.remove(components[components.len() - 1]);
    }
}

fn try_index_path_mut<'a, 'b>(root: &'a mut V, path: impl IntoIterator<Item = &'b V>) -> Option<&'a mut V> {
    let mut current = &mut *root;
    for key in path.into_iter() {
        if let Some(m) = &mut current.0
            && let Some(next) = m.get_mut(key)
        {
            current = next;
        } else {
            return None;
        }
    }
    Some(current)
}

/* =========================
   NAMESPACE AND TRIGGER ENFORCEMENT
========================= */

// .sys.x. or .var....
fn panic_if_write_path_is_not_dot_sys_dot_x_dot_or_dot_var_dot_whatever(components: &[&V]) {
    if components.is_empty() {
        panic!("cannot replace root: writes must go through a namespace");
    }
    let first = value_as_string(components[0])
        .unwrap_or_else(|| panic!("first path component must be a string namespace"));
    match first.as_str() {
        "sys" => {
            if components.len() != 2 {
                panic!("cannot write to .sys. directly. can only write to .sys.run.");
            }
            let opname = value_as_string(components[1])
                .unwrap_or_else(|| panic!("second pathstep must be string run"));
            if opname != "run" {
                panic!("can only be in run in sys for now");
            }
        }
        "var" => {}
        other => panic!("forbidden namespace: '{}' (allowed: ops, sys)", other),
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
    if components.len() != 2 { return Ok(()); }
    if value_as_string(components[0]).as_deref() != Some("sys") { return Ok(()); }
    if value_as_string(components[1]).as_deref() != Some("run") { return Ok(()); }
    fire_op(root, written)
}

fn fire_op(root: &mut V, written: &V) -> Result<(), InterpError> {
    // let from_str = |inner: &str| {
    //     let mut chars = inner.chars();
    //     V::marked_list_counted({
    //         std::iter::from_fn(|| decode_escaped_char(&mut chars).map(V::from_prim))
    //     })
    // };
    // let path = &[str_to_value("sys"), str_to_value("run"), ().into()];
    // let Some(opname) = value_as_string(walk(root, path)) else {
    //     return Ok(());
    // };
    // let op_fn = crate::ops::op_registry(&opname).unwrap();
    // let args_path = build_path(&["ops", opname, "args"]);
    // let args = walk(root, args_path.iter::<MarkedList>()).clone();
    // let result = op_fn(root, &args)?;
    // let return_path = build_path(&["ops", opname, "return"]);
    // execute_an_assignment_which_might_delete_instead_and_might_fire(root, &return_path, result)
    todo!()
}

/* =========================
   STATEMENT EXECUTION
========================= */

fn exec(root: &mut V, instructions: &V) -> Result<(), InterpError> {
    for statement in instructions.iterate_where_self_is_sys_list::<true>() {
        let (lhs, rhs) = Assignment::resolve_from_value(statement).resolve(root).into_inner();

        let (lhs, rhs) = (lhs.clone(), rhs.clone());

        if let Err(e) = execute_an_assignment_which_might_delete_instead_and_might_fire(root, &lhs, rhs) {
            eprintln!("runtime error in {:?}: {:?}", statement, e);
        }
    }
    Chennamallikarjuna.flowers()
}


struct Chennamallikarjuna;
impl Chennamallikarjuna {
    fn flowers<E>(self) -> Result<(), E> {
        Ok(())
    }
}

struct Atomic(u32);
macro_rules! from_primitive_atomic {
    ($($($ty:ty),+ $(,)?)?) => {
        $($(
            impl From<$ty> for Atomic {
                fn from(value: $ty) -> Self {
                    Self(value as u32)
                }
            }
        )+)?
    };
}
from_primitive_atomic! { u8, u16, u32, usize }
impl TryFrom<&V> for Atomic {
    type Error = String;
    fn try_from(value: &V) -> Result<Self, Self::Error> {
        let n = 'n: {
            let mut got = match &value.0 {
                Some(n) => n,
                None => break 'n 0,
            };
            let mut i = 1u32;
            loop {
                if got.len() == 0 {
                    break 'n i;
                }
                if got.len() > 1 {
                    return Err("map size > 1".into());
                }
                got = &got.get(&Atomic(0).into()).ok_or::<String>("map without key self".into())?[()];
                i += 1;
            }
        };
        Ok(n.into())
    }
}

struct Assignment<'a> {
    lhs_path: &'a V,
    lhs_depth: u32,
    rhs_path: &'a V,
    rhs_depth: u32,
}

struct ResolvedAssignment<'a> {
    lhs: &'a V,
    rhs: &'a V,
}
impl<'a> ResolvedAssignment<'a> {
    fn into_inner(self) -> (&'a V, &'a V) {
        (self.lhs, self.rhs)
    }
}
impl<'a> Assignment<'a> {
    fn resolve(self, root: &'a V) -> ResolvedAssignment<'a> {
        ResolvedAssignment {
            lhs: self.lhs_path,
            rhs: self.rhs_path,
        }
            .release(|ra| {
                ra.lhs.with_root(root).resolve_dereferencing(self.lhs_depth);
                ra.rhs.with_root(root).resolve_dereferencing(self.rhs_depth);
            })
    }
    fn resolve_from_value(value: &'a V) -> Self {
        let Some(map) = &value.0 else {
            panic!("top-level assignment is not map: {:?}", value);
        };
        let Some(lhs_pair) = map.get_self() else {
            panic!("no lhs in assignment: {:?}", value);
        };
        let Some(rhs_pair) = map.get_atomic_one() else {
            panic!("no rhs in assignment: {:?}", value);
        };
        fn extract_pair_value<'a>(pair: &'a V, side: &str) -> (&'a V, u32) {
            let Some(m) = &pair.0 else {
                panic!("{} in assignment is not a map: {:?}", side, pair);
            };
            let [expr, depth] = std::array::from_fn(|i| match
                m.get(&Atomic::from(i).into())
                {
                    Some(got) => got,
                    None => panic!("no {} in {} of assignment: {:?}", ["expr", "depth"][i], side, pair),
                }
            );
            let Ok(Atomic(depth)) = depth.try_into() else {
                panic!("{} depth of assignment is invalid atomic: {:?}", side, depth);
            };
            (expr, depth)
        }
        let ((lhs_path, lhs_depth), (rhs_path, rhs_depth))
            = (extract_pair_value(lhs_pair, "lhs"), extract_pair_value(rhs_pair, "rhs"));
        Self { lhs_path, lhs_depth, rhs_path, rhs_depth }
    }
}


fn resolve_dereferencing_via_depth<'a>(current: &mut &'a V, root: &'a V, depth: u32) {
    for _ in 0 .. depth {
        *current = walk(root, current.iter::<LinkedList>());
    }
}

struct WithRoot<'a, T> {
    item: T,
    root: &'a V,
}
impl<'a, 'b> WithRoot<'a, &'b mut &'a V> {
    fn resolve_dereferencing(self, depth: u32) {
        for _ in 0 .. depth {
            *self.item = walk(self.root, self.item.iter::<LinkedList>());
        }
    }
}


trait X {
    fn some(self) -> Option<Self>
    where
        Self: Sized,
    {
        Some(self)
    }
    fn some_ref(&self) -> Option<&Self> {
        Some(&self)
    }
    fn release<F: FnOnce(&mut Self)>(mut self, f: F) -> Self
    where 
        Self: Sized,
    {
        f(&mut self);
        self
    }
    fn with_root<'a, 'b>(&'b mut self, root: &'a V) -> WithRoot<'a, &'b mut Self> {
        WithRoot {
            item: self,
            root,
        }
    }
    fn try_into_t<U>(self) -> Result<U, <Self as TryInto<U>>::Error>
    where
        Self: TryInto<U>,
    {
        self.try_into()
    }
}
impl<T> X for T { }




/* =========================
   PRE-INITIALIZATION
========================= */

fn preinit_ops(root: &mut V) {
    for &name in crate::ops::registered_op_names() {
        ensure_path(root, &["ops", name, "trigger"], L::new().into());
    }
}

// val must be non-self
fn doing_the_actual_assignment_no_more_checks(root: &mut V, path: impl IntoIterator<Item = V>, val: V) {
    debug_assert!(val.0.is_some());
    let mut current = root;
    for v in path.into_iter() {
        if current.0.is_none() {
            current.0.insert(L::new());
        }
        current = current[()].entry(v).or_insert(L::new().into());
    }
    *current = val;
}

fn ensure_path(root: &mut V, components: &[&str], val: V) {
    doing_the_actual_assignment_no_more_checks(
        root,
        components.iter().map(|s| str_to_value(s)),
        val,
    );
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
    V::list::<LinkedList>(
        components.iter().map(|s| str_to_value(*s))
    )
}

pub(crate) fn str_to_value(s: &str) -> V {
    V::list::<MarkedList>(
        s.chars().map(|c| V::from_prim(c as u32))
    )
}

fn value_as_string(v: &V) -> Option<String> {
    let m = v.0.as_ref()?;
    let len = m.get_self()?.try_into_t::<Atomic>().ok()?.0;
    if m.len() != len as usize + 1 {
        return None;
    }
    (1 ..= len)
        .map(|i| char::from_u32(
            m.get_atomic(i)?.try_to_prim()?
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
    if let Ok(Atomic(n)) = v.try_into() {
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
                print_value(&m.get_atomic(i).unwrap_or(SELF_REF));
            }
            print!(".");
        }
    } else if is_indexed_unmarked(m) {
        print!("[");
        for i in 0 .. m.len() as u32 {
            if i > 0 {
                print!(", ");
            }
            print_value(&m[Atomic(i)]);
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
    (0 .. (m.len() as u32)).all(|i| m.contains_key(Atomic(i)))
}
fn is_indexed_marked(m: &L) -> Option<usize> {
    let len = m.get_self()?.try_into_t::<Atomic>().ok()?.0;
    m
        .keys()
        .all(|key| key.try_into_t::<Atomic>().is_ok_and(|Atomic(i)| i <= len))
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
