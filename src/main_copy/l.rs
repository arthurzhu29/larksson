#![allow(unused)]

use super::*;
use std::collections::btree_map::{Iter, Keys, ValuesMut};

#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct L(BTreeMap<V, V>);

impl From<L> for V {
    fn from(value: L) -> Self {
        Self(Some(value))
    }
}

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
trait Gettable {
    fn get(self, host: &L) -> Option<&V>;
    fn contains_key(self, host: &L) -> bool;
}
impl Gettable for &V {
    fn get(self, host: &L) -> Option<&V> {
        host.0.get(self)
    }
    fn contains_key(self, host: &L) -> bool {
        host.0.contains_key(self)
    }
}
impl Gettable for Atomic {
    fn get(self, host: &L) -> Option<&V> {
        host.0.get(&self.into())
    }
    fn contains_key(self, host: &L) -> bool {
        host.0.contains_key(&self.into())
    }
}
impl L {
    pub fn get_atomic(&self, atomic: u32) -> Option<&V> {
        self.get(Atomic(atomic))
    }
    pub fn get_self(&self) -> Option<&V> {
        self.get(Atomic(0))
    }
    pub fn get_atomic_one(&self) -> Option<&V> {
        self.get(Atomic(1))
    }
}
impl L {
    pub fn new() -> Self {
        L(BTreeMap::new())
    }

    pub fn insert(&mut self, key: V, val: V) {
        if val.0.is_some() {
            self.0.insert(key, val);
        }
    }

    pub fn get<T: Gettable>(&self, key: T) -> Option<&V> {
        key.get(self)
    }

    pub fn get_mut(&mut self, key: &V) -> Option<&mut V> {
        self.0.get_mut(key)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn entry(&mut self, key: V) -> std::collections::btree_map::Entry<V, V> {
        self.0.entry(key)
    }

    pub fn remove(&mut self, key: &V) -> Option<V> {
        self.0.remove(key)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains_key<T: Gettable>(&self, key: T) -> bool {
        key.contains_key(self)
    }

    pub fn keys(&self) -> Keys<V, V> {
        self.0.keys()
    }

    pub fn iter(&self) -> Iter<V, V> {
        self.0.iter()
    }

    pub fn retain<F: FnMut(&V, &mut V) -> bool>(&mut self, f: F) {
        self.0.retain(f);
    }

    pub fn values_mut(&mut self) -> ValuesMut<V, V> {
        self.0.values_mut()
    }
}
impl FromIterator<(V, V)> for L {
    fn from_iter<T: IntoIterator<Item = (V, V)>>(iter: T) -> Self {
        L(BTreeMap::from_iter(iter))
    }
}
impl IntoIterator for L {
    type Item = <BTreeMap<V, V> as IntoIterator>::Item;
    type IntoIter = <BTreeMap<V, V> as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl<'a> IntoIterator for &'a L {
    type Item = <&'a BTreeMap<V, V> as IntoIterator>::Item;
    type IntoIter = <&'a BTreeMap<V, V> as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (&self.0).into_iter()
    }
}
impl std::ops::Index<&V> for L {
    type Output = V;
    fn index(&self, index: &V) -> &Self::Output {
        self.0.get(index).unwrap_or(&V(None))
    }
}
impl std::ops::Index<Atomic> for L {
    type Output = V;
    fn index(&self, index: Atomic) -> &Self::Output {
        self.0.get(&index.into()).unwrap_or(&V(None))
    }
}