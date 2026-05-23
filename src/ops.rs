use super::*;

type OpFn = fn(&mut MyValue, &MyValue) -> Result<MyValue, InterpError>;

pub fn op_registry(name: &str) -> Option<OpFn> {
    match name {
        "add" => Some(op_add),
        "run" => Some(op_run),
        _ => None,
    }
}

fn op_add(_root: &mut MyValue, args: &MyValue) -> Result<MyValue, InterpError> {
    let MyValue::Map(m) = args else { panic!("op args must be a map"); };
    let left  = m.get(&str_to_myvalue("left"))
        .unwrap_or_else(|| panic!("add: missing arg 'left'"));
    let right = m.get(&str_to_myvalue("right"))
        .unwrap_or_else(|| panic!("add: missing arg 'right'"));
    let MyValue::Val(l) = left  else { panic!("add: 'left' must be a number"); };
    let MyValue::Val(r) = right else { panic!("add: 'right' must be a number"); };
    Ok(MyValue::Val(l + r))
}

fn op_run(root: &mut MyValue, args: &MyValue) -> Result<MyValue, InterpError> {
    let MyValue::Map(m) = args else { panic!("op args must be a map"); };
    let program = m.get(&str_to_myvalue("program"))
        .unwrap_or_else(|| panic!("run: missing arg 'program'"));
    exec_mv(root, program)?;
    Ok(MyValue::Val(0)) // run has no meaningful return; placeholder
}