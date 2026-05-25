#![allow(unused)]

use super::*;


pub trait ValueIteratorMarker {
    type IntoIter<'a>: ValueIterator<'a>;
}
pub trait ValueFromIteratorMarker<T: IntoIterator<Item = V>> {
    fn from_iter(iter: T) -> V;
}
pub trait ValueIterator<'a>: Iterator<Item = &'a V> {
    fn init(v: &'a V) -> Self;
}
pub trait GetList<T: IntoIterator<Item = V>> {
    fn list<M: ValueFromIteratorMarker<T>>(list: T) -> V;
}
impl<T: IntoIterator<Item = V>> GetList<T> for V {
    fn list<M: ValueFromIteratorMarker<T>>(list: T) -> V {
        M::from_iter(list)
    }
}
impl V {
    pub fn iter<'a, M: ValueIteratorMarker>(&'a self) -> M::IntoIter<'a> {
        M::IntoIter::init(self)
    }
}

//////////////////////////////////// MarkedList //////////////////////////////////////////

pub struct MarkedList;
impl ValueIteratorMarker for MarkedList {
    type IntoIter<'a> = MarkedListIter<'a>;
}
impl<T: IntoIterator<Item = V>> ValueFromIteratorMarker<T> for MarkedList {
    fn from_iter(iter: T) -> V {
        let mut n = 0usize;
        let mut m = iter
            .into_iter()
            .map(|val| (V::from_prim(increment(&mut n)), val))
            .collect::<L>();
        m.insert(().into(), V::from_prim(n));
        m.into()
    }
}
pub struct MarkedListIter<'a> {
    range: std::ops::Range<usize>,
    m: Option<&'a L>,
}
impl<'a> ValueIterator<'a> for MarkedListIter<'a> {
    fn init(item: &'a V) -> Self {
        MarkedListIter {
            range: 0 .. item.get_marked_list_len().unwrap_or(0),
            m: item.0.as_ref(),
        }
    }
}
impl<'a> Iterator for MarkedListIter<'a> {
    type Item = &'a V;
    fn next(&mut self) -> Option<Self::Item> {
        self
            .m?
            .get(&V::from_prim(self.range.next()?))
            .unwrap_or(SELF_REF)
            .some()
    }
}

//////////////////////////////////// SysList ////////////////////////////////////

pub struct SysList<const MUST_BE_MAP: bool = true>;
impl<const MUST_BE_MAP: bool> ValueIteratorMarker for SysList<MUST_BE_MAP> {
    type IntoIter<'a> = SysListIter<'a, MUST_BE_MAP>;
}
impl<T: IntoIterator<Item = V>> ValueFromIteratorMarker<T> for SysList<true> {
    fn from_iter(iter: T) -> V {
        iter.into_iter()
            .enumerate()
            .map(|(i, val)| (Atomic(i as u32).into(), val))
            .collect::<L>()
            .into()
    }
}
pub struct SysListIter<'a, const MUST_BE_MAP: bool = true> {
    current: u32,
    done: bool,
    map: Option<&'a L>,
}
impl<'a, const MUST_BE_MAP: bool> ValueIterator<'a> for SysListIter<'a, MUST_BE_MAP> {
    fn init(v: &'a V) -> Self {
        SysListIter {
            current: 0,
            done: false,
            map: v.0.as_ref(),
        }
    }
}
impl<'a, const MUST_BE_MAP: bool> Iterator for SysListIter<'a, MUST_BE_MAP> {
    type Item = &'a V;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let Some(map) = self.map else {
            if MUST_BE_MAP {
                panic!("sys list not map");
            }
            self.done = true;
            return None;
        };
        let current = self.current;
        self.current += 1;
        let got = map.get_atomic(current);
        if got.is_none() {
            self.done = true;
        }
        got
    }
}

//////////////////////////////////// LinkedList //////////////////////////////////
// empty -> <>
// item -> { <>: item }
// .a.b. -> { <>: a, {}: { <>: b }}

pub struct LinkedList;
impl ValueIteratorMarker for LinkedList {
    type IntoIter<'a> = LinkedListIter<'a>;
}
impl<T: IntoIterator<Item = V>> ValueFromIteratorMarker<T> for LinkedList
where
    T::IntoIter: DoubleEndedIterator,
{
    fn from_iter(iter: T) -> V {
        let mut got = ().into();
        for item in iter.into_iter().rev() {
            got = L::from([(().into(), item), (L::new().into(), got)]).into()
        }
        got
    }
}
pub struct LinkedListIter<'a> {
    current: &'a V,
}
impl<'a> ValueIterator<'a> for LinkedListIter<'a> {
    fn init(v: &'a V) -> Self {
        Self { current: v }
    }
}
impl<'a> Iterator for LinkedListIter<'a> {
    type Item = &'a V;
    fn next(&mut self) -> Option<Self::Item> {
        let Some(map) = &self.current.0 else {
            return None;
        };
        self.current = &map[&().into()];
        map[&L::new().into()].some_ref()
    }
}