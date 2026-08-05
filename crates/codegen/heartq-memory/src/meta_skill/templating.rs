//! when / route evaluation and step input rendering (OpenSquilla-aligned).
//!
//! Uses a restricted [`minijinja`] environment so expressions like
//! `outputs.classify == 'URL'` match OpenSquilla's Jinja semantics.

use minijinja::{context, Environment, Value as JinjaValue};
use serde_json::Map;

use super::model::{MetaSkillStep, RouteCase};

fn jinja_env() -> Environment<'static> {
    let mut env = Environment::new();
    // Keep undefined quiet so missing keys evaluate falsy rather than hard-fail
    // for skip-style when expressions; syntax errors still surface.
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
    env
}

fn context_from(
    inputs: &Map<String, serde_json::Value>,
    outputs: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<JinjaValue, String> {
    let inputs_v = serde_json::to_value(inputs).map_err(|e| e.to_string())?;
    let outputs_v = serde_json::to_value(outputs).map_err(|e| e.to_string())?;
    let state_v = outputs_v.clone();
    Ok(context! {
        inputs => inputs_v,
        outputs => outputs_v,
        state => state_v,
    })
}

fn is_truthy(v: &JinjaValue) -> bool {
    if v.is_undefined() || v.is_none() {
        return false;
    }
    v.is_true()
}

/// Evaluate a step-level `when` expression.
///
/// Empty / `"always"` → true; `"never"` → false; otherwise Jinja expression.
pub fn eval_when(
    when: Option<&str>,
    inputs: &Map<String, serde_json::Value>,
    outputs: &std::collections::HashMap<String, serde_json::Value>,
) -> bool {
    match when.map(str::trim).filter(|s| !s.is_empty()) {
        None | Some("always") => true,
        Some("never") => false,
        Some(expr) => match eval_bool_expr(expr, inputs, outputs) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, expr, "meta_skill when eval failed; treating as false");
                false
            }
        },
    }
}

fn eval_bool_expr(
    expr: &str,
    inputs: &Map<String, serde_json::Value>,
    outputs: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<bool, String> {
    let env = jinja_env();
    let compiled = env.compile_expression(expr).map_err(|e| e.to_string())?;
    let ctx = context_from(inputs, outputs)?;
    let value = compiled.eval(ctx).map_err(|e| e.to_string())?;
    Ok(is_truthy(&value))
}

/// Return the first matching route `to`, or `None` to keep the default skill.
pub fn resolve_route(
    cases: &[RouteCase],
    inputs: &Map<String, serde_json::Value>,
    outputs: &std::collections::HashMap<String, serde_json::Value>,
) -> Option<String> {
    if cases.is_empty() {
        return None;
    }
    for case in cases {
        match eval_bool_expr(&case.when, inputs, outputs) {
            Ok(true) => return Some(case.to.clone()),
            Ok(false) => continue,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    when = %case.when,
                    "meta_skill route when eval failed; skipping case"
                );
                continue;
            }
        }
    }
    None
}

/// Render string values in step inputs as Jinja templates.
pub fn render_inputs(
    step: &MetaSkillStep,
    inputs: &Map<String, serde_json::Value>,
    outputs: &std::collections::HashMap<String, serde_json::Value>,
) -> Map<String, serde_json::Value> {
    let env = jinja_env();
    let ctx = match context_from(inputs, outputs) {
        Ok(c) => c,
        Err(_) => return step.inputs.clone(),
    };
    let mut out = Map::new();
    for (k, v) in &step.inputs {
        match v {
            serde_json::Value::String(s) if s.contains("{{") || s.contains("{%") => {
                match env.render_str(s, ctx.clone()) {
                    Ok(rendered) => {
                        out.insert(k.clone(), serde_json::Value::String(rendered));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, key = %k, "meta_skill input template failed");
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            other => {
                out.insert(k.clone(), other.clone());
            }
        }
    }
    out
}

/// Effective skill name after applying route cases.
pub fn effective_skill(
    step: &MetaSkillStep,
    inputs: &Map<String, serde_json::Value>,
    outputs: &std::collections::HashMap<String, serde_json::Value>,
) -> String {
    resolve_route(&step.route, inputs, outputs).unwrap_or_else(|| step.skill_name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn when_always_never_empty() {
        let inputs = Map::new();
        let outputs = HashMap::new();
        assert!(eval_when(None, &inputs, &outputs));
        assert!(eval_when(Some("always"), &inputs, &outputs));
        assert!(!eval_when(Some("never"), &inputs, &outputs));
    }

    #[test]
    fn when_expr_reads_outputs() {
        let inputs = Map::new();
        let mut outputs = HashMap::new();
        outputs.insert("classify".into(), serde_json::json!("URL"));
        assert!(eval_when(
            Some("outputs.classify == 'URL'"),
            &inputs,
            &outputs
        ));
        assert!(!eval_when(
            Some("outputs.classify == 'TEXT'"),
            &inputs,
            &outputs
        ));
    }

    #[test]
    fn route_first_match_wins() {
        let inputs = Map::new();
        let mut outputs = HashMap::new();
        outputs.insert("kind".into(), serde_json::json!("a"));
        let cases = vec![
            RouteCase {
                when: "outputs.kind == 'a'".into(),
                to: "skill-a".into(),
            },
            RouteCase {
                when: "true".into(),
                to: "skill-b".into(),
            },
        ];
        assert_eq!(
            resolve_route(&cases, &inputs, &outputs).as_deref(),
            Some("skill-a")
        );
    }
}
