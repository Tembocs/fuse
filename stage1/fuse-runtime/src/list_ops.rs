//! List<T> operations for the Fuse runtime.

use crate::value::FuseValue;

/// Retain elements where predicate returns true (in-place mutation).
pub fn fuse_list_retain_where(list: &mut FuseValue, pred: &dyn Fn(&FuseValue) -> bool) {
    let elems = list.as_list_mut();
    elems.retain(|e| pred(e));
}

/// Map a function over a list, returning a new list.
pub fn fuse_list_map(list: &FuseValue, f: &dyn Fn(&FuseValue) -> FuseValue) -> FuseValue {
    FuseValue::List(list.as_list().iter().map(f).collect())
}

/// Filter a list by predicate.
pub fn fuse_list_filter(list: &FuseValue, pred: &dyn Fn(&FuseValue) -> bool) -> FuseValue {
    FuseValue::List(list.as_list().iter().filter(|e| pred(e)).cloned().collect())
}

/// Sort a list of comparable values.
pub fn fuse_list_sorted(list: &FuseValue) -> FuseValue {
    let mut elems = list.as_list().clone();
    elems.sort_by(|a, b| {
        match (a, b) {
            (FuseValue::Int(x), FuseValue::Int(y)) => x.cmp(y),
            (FuseValue::Float(x), FuseValue::Float(y)) => x.partial_cmp(y).unwrap(),
            (FuseValue::Str(x), FuseValue::Str(y)) => x.cmp(y),
            _ => std::cmp::Ordering::Equal,
        }
    });
    FuseValue::List(elems)
}

pub fn fuse_list_first(list: &FuseValue) -> FuseValue {
    let elems = list.as_list();
    if let Some(e) = elems.first() { FuseValue::some(e.clone()) } else { FuseValue::none() }
}

pub fn fuse_list_last(list: &FuseValue) -> FuseValue {
    let elems = list.as_list();
    elems.last().cloned().unwrap_or(FuseValue::none())
}

pub fn fuse_list_len(list: &FuseValue) -> FuseValue {
    FuseValue::Int(list.as_list().len() as i64)
}

pub fn fuse_list_is_empty(list: &FuseValue) -> FuseValue {
    FuseValue::Bool(list.as_list().is_empty())
}

pub fn fuse_list_sum(list: &FuseValue) -> FuseValue {
    let elems = list.as_list();
    if elems.is_empty() { return FuseValue::Int(0); }
    match &elems[0] {
        FuseValue::Int(_) => FuseValue::Int(elems.iter().map(|e| e.as_int()).sum()),
        FuseValue::Float(_) => FuseValue::Float(elems.iter().map(|e| e.as_float()).sum()),
        _ => panic!("cannot sum non-numeric list"),
    }
}

pub fn fuse_list_push(list: &mut FuseValue, val: FuseValue) {
    list.as_list_mut().push(val);
}
