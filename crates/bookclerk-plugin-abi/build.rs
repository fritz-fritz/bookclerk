//! Compile plugin Cap'n Proto schemas into Rust (Cap'n Proto RPC interfaces).

fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/plugin_v2.capnp")
        .file("schema/plugin_v2_host.capnp")
        .run()
        .expect("compile plugin capnp schemas");
}
