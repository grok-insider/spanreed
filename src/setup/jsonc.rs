//! Edit one reviewed provider while retaining surrounding JSONC text.
use jsonc_parser::{
    cst::{CstInputValue, CstRootNode},
    ParseOptions,
};
use serde_json::Value;
fn options() -> ParseOptions {
    ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
    }
}
pub(super) fn parse(text: &str) -> Result<Value, String> {
    jsonc_parser::parse_to_serde_value(text, &options())
        .map_err(|_| "Invalid JSONC configuration".into())
}
fn input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(values) => CstInputValue::Array(values.iter().map(input).collect()),
        Value::Object(values) => CstInputValue::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), input(value)))
                .collect(),
        ),
    }
}
pub(super) fn edit(
    text: &str,
    provider: &str,
    addition: &Value,
    expected: &Value,
) -> Result<Vec<u8>, String> {
    let root = CstRootNode::parse(text, &options()).map_err(|_| "Invalid JSONC configuration")?;
    let object = root
        .object_value()
        .ok_or("Configuration must be an object")?;
    let providers = object
        .object_value_or_create("provider")
        .ok_or("Provider configuration must be an object")?;
    if let Some(property) = providers.get(provider) {
        if addition.is_null() {
            property.remove();
        } else {
            property.set_value(input(addition));
        }
    } else if !addition.is_null() {
        providers.append(provider, input(addition));
    }
    let text = root.to_string();
    if parse(&text)? != *expected {
        return Err("JSONC edit differs from approved change".into());
    }
    Ok(text.into_bytes())
}
