use std::sync::LazyLock;

use serde_json::Value;

static NULL: LazyLock<Value> = LazyLock::new(|| Value::Null);

/// The value at a JSON pointer such as `/data/repository/id`, and Null when any step is missing,
/// which is what indexing a `Value` gives without its panic on a bad assignment.
pub fn at<'a>(value: &'a Value, pointer: &str) -> &'a Value {
    value.pointer(pointer).unwrap_or(&NULL)
}
