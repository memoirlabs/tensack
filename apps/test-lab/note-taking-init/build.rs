use std::env;
use std::fs;
use std::path::PathBuf;

use tensack_schema_compiler::{compile_schema, emit_raw_rust};

fn main() {
    println!("cargo:rerun-if-changed=schema.tensack");

    let schema_source = fs::read_to_string("schema.tensack").expect("read schema.tensack");
    let ir = compile_schema(&schema_source).expect("compile schema.tensack");
    let generated = emit_raw_rust(&ir);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out_dir.join("tensack_generated_schema.rs"), generated)
        .expect("write generated schema SDK");
}
