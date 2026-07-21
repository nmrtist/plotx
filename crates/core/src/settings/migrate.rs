use serde_json::{Value, json};

pub fn migrate(value: Value, from: u32) -> Value {
    if from == 0 && is_legacy_preferences(&value) {
        return v0_to_v1(value);
    }
    value
}

fn is_legacy_preferences(value: &Value) -> bool {
    let Value::Object(object) = value else {
        return false;
    };
    (object.contains_key("include_view_snapshots") || object.contains_key("snap_enabled"))
        && !object.contains_key("general")
        && !object.contains_key("export")
}

fn v0_to_v1(value: Value) -> Value {
    let include_view_snapshots = value
        .get("include_view_snapshots")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let snap_enabled = value
        .get("snap_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    json!({
        "schema_version": 1,
        "general": {
            "snap_enabled": snap_enabled
        },
        "export": {
            "include_view_snapshots": include_view_snapshots
        }
    })
}
