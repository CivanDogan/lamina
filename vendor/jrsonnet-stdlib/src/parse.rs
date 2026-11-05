use jrsonnet_evaluator::{IStr, Result, Val, error, function::builtin};

#[builtin]
pub fn builtin_parse_json(str: IStr) -> Result<Val> {
	let value: Val = serde_json::from_str(&str).map_err(|e| error!("failed to parse json: {e}"))?;
	Ok(value)
}

// Lamina: YAML parsing removed with the serde-saphyr dependency.
#[builtin]
pub fn builtin_parse_yaml(str: IStr) -> Result<Val> {
	let _ = str;
	Err(error!("std.parseYaml is not available in Lamina"))
}
