

use super::*;

type OpFn = fn(&MyValue) -> Result<MyValue, InterpError>;

pub fn op_registry(name: &str) -> Option<OpFn> {
    match name {
        "add" => Some(op_add),
        _ => None,
    }
}

fn op_add(args: &MyValue) -> Result<MyValue, InterpError> {
    let MyValue::Map(m) = args else { panic!("op args must be a map"); };
    let left  = m.get(&str_to_myvalue("left"))
        .unwrap_or_else(|| panic!("add: missing arg 'left'"));
    let right = m.get(&str_to_myvalue("right"))
        .unwrap_or_else(|| panic!("add: missing arg 'right'"));
    let MyValue::Val(l) = left  else { panic!("add: 'left' must be a number"); };
    let MyValue::Val(r) = right else { panic!("add: 'right' must be a number"); };
    Ok(MyValue::Val(l + r))
}