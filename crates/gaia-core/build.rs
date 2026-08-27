//! contracts/ を読み、(1) 外部 $ref を局所化して $defs を同梱した自己完結スキーマの束 contracts.json、
//! (2) typify による Rust 型 contract_types.rs を OUT_DIR に生成する。契約の誤りはビルドエラーにする。
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value, json};
use typify::{TypeSpace, TypeSpaceSettings};

/// `../defs/common.json#/$defs/X` → `#/$defs/X`
fn localize_refs(v: &mut Value) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get_mut("$ref")
                && let Some(idx) = r.find('#')
                && idx > 0
            {
                *r = r[idx..].to_string();
            }
            for (_, child) in map.iter_mut() {
                localize_refs(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(localize_refs),
        _ => {}
    }
}

/// スキーマが推移的に参照する $defs 名を集める。
fn collect_refs(v: &Value, pool: &Map<String, Value>, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref")
                && let Some(name) = r.strip_prefix("#/$defs/")
                && out.insert(name.to_string())
            {
                let def = pool
                    .get(name)
                    .unwrap_or_else(|| panic!("$ref to unknown $defs `{name}`"));
                collect_refs(def, pool, out);
            }
            for child in map.values() {
                collect_refs(child, pool, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_refs(i, pool, out)),
        _ => {}
    }
}

/// 参照している $defs だけを同梱した自己完結スキーマを返す。
fn self_contained(schema: &Value, pool: &Map<String, Value>) -> Value {
    let mut used = BTreeSet::new();
    collect_refs(schema, pool, &mut used);
    let mut out = schema.clone();
    if !used.is_empty() {
        let defs: Map<String, Value> = used
            .into_iter()
            .map(|n| (n.clone(), pool[&n].clone()))
            .collect();
        out["$defs"] = Value::Object(defs);
    }
    out
}

fn pascal(s: &str) -> String {
    s.split(['_', '-', '.'])
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn read_json(path: &Path) -> Value {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let contracts = manifest_dir.join("../../contracts");
    println!("cargo:rerun-if-changed={}", contracts.display());

    let manifest = read_json(&contracts.join("manifest.json"));
    let mut common = read_json(&contracts.join(manifest["defs"].as_str().expect("manifest.defs")));
    localize_refs(&mut common);
    let pool: Map<String, Value> = common["$defs"]
        .as_object()
        .expect("common.json $defs")
        .clone();

    let mut bundle_tools = Vec::new();
    let mut typed: Vec<(String, Value)> = Vec::new();
    for t in manifest["tools"].as_array().expect("manifest.tools") {
        let name = t["name"].as_str().expect("tool.name");
        let file = contracts.join(t["file"].as_str().expect("tool.file"));
        let mut tool = read_json(&file);
        assert_eq!(
            tool["name"].as_str(),
            Some(name),
            "{}: name mismatch with manifest",
            file.display()
        );
        localize_refs(&mut tool);
        assert!(
            tool.get("$defs").is_none(),
            "{}: tool files must not define $defs",
            file.display()
        );

        let input = tool
            .get("inputSchema")
            .unwrap_or_else(|| panic!("{}: inputSchema missing", file.display()));
        assert_eq!(
            input["type"].as_str(),
            Some("object"),
            "{}: inputSchema.type must be object",
            file.display()
        );
        let input_sc = self_contained(input, &pool);
        typed.push((format!("{}Input", pascal(name)), input_sc.clone()));

        let output_sc = tool.get("outputSchema").map(|o| self_contained(o, &pool));
        if let Some(o) = &output_sc {
            typed.push((format!("{}Output", pascal(name)), o.clone()));
        }

        bundle_tools.push(json!({
            "name": name,
            "title": tool.get("title"),
            "description": tool["description"].as_str().unwrap_or_else(|| panic!("{}: description missing", file.display())),
            "roles": t["roles"],
            "enabled": t["enabled"].as_bool().unwrap_or(true),
            "annotations": tool.get("annotations").cloned().unwrap_or(json!({})),
            "inputSchema": input_sc,
            "outputSchema": output_sc,
        }));
    }
    let bundle = json!({
        "contract_version": manifest["contract_version"],
        "server_name": manifest["server_name"],
        "tools": bundle_tools,
    });

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(
        out_dir.join("contracts.json"),
        serde_json::to_string_pretty(&bundle).unwrap(),
    )
    .unwrap();

    // typify: 共通 $defs を 1 回だけ登録してから、ツールごとの Input/Output を名前付きで登録する。
    let mut settings = TypeSpaceSettings::default();
    settings
        .with_struct_builder(false)
        .with_derive("PartialEq".to_string());
    let mut ts = TypeSpace::new(&settings);
    ts.add_ref_types(pool.iter().map(|(k, v)| {
        (
            k.clone(),
            serde_json::from_value::<schemars::schema::Schema>(v.clone())
                .unwrap_or_else(|e| panic!("$defs.{k} is not a valid schema: {e}")),
        )
    }))
    .expect("add common $defs");
    for (type_name, mut schema) in typed {
        // $defs は共通プールで登録済みなので、typify に渡す前に外す
        if let Value::Object(m) = &mut schema {
            m.remove("$defs");
        }
        let schema: schemars::schema::Schema = serde_json::from_value(schema)
            .unwrap_or_else(|e| panic!("{type_name}: invalid schema: {e}"));
        ts.add_type_with_name(&schema, Some(type_name.clone()))
            .unwrap_or_else(|e| panic!("{type_name}: typify failed: {e}"));
    }
    let code = prettyplease::unparse(
        &syn::parse2::<syn::File>(ts.to_stream()).expect("generated code parses"),
    );
    fs::write(out_dir.join("contract_types.rs"), code).unwrap();
    assert!(
        !ts.uses_regress(),
        "contracts must not use `pattern` (would require the regress crate)"
    );
    assert!(
        !ts.uses_chrono(),
        "contracts must not use `format: date-time` (would require chrono)"
    );
}
