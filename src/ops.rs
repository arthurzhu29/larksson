use super::*;

type OpFn = fn(&mut Value, &Value) -> Result<Value, InterpError>;

pub fn op_registry(name: &str) -> Option<OpFn> {
    match name {
        "add" => Some(op_add),
        "run" => Some(op_run),
        _ => None,
    }
}

pub fn registered_op_names() -> &'static [&'static str] {
    &["add", "run"]
}

fn op_add(_root: &mut Value, args: &Value) -> Result<Value, InterpError> {
    let left  = lookup(args, "left");
    let right = lookup(args, "right");
    let l = left.try_to_prim::<u32>()
        .unwrap_or_else(|| panic!("add: 'left' is not a number, got {:?}", left));
    let r = right.try_to_prim::<u32>()
        .unwrap_or_else(|| panic!("add: 'right' is not a number, got {:?}", right));
    Ok(Value::atomic(l + r))
}

fn op_run(root: &mut Value, args: &Value) -> Result<Value, InterpError> {
    let program = lookup(args, "program");
    exec(root, program)?;
    // run has no meaningful return value. Returning Self_ causes
    // .ops.run.return. to be removed (write-of-self → delete).
    Ok(Value::SelfSentinel)
}
