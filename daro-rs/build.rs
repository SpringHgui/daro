fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icons/ui/logo.ico");
    println!("cargo:rerun-if-changed=assets/tabler");
    emit_tabler_assets();
    let _ = embed_resource::compile("assets/app.rc", embed_resource::NONE);
}

/// 把 assets/tabler 下的 SVG 清单烘成静态数组，供 tabler_assets 模块内嵌。
fn emit_tabler_assets() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/tabler");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".svg"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let mut out = String::from("// 由 build.rs 自动生成，勿手改。\nstatic FILES: [(&str, &[u8]); ");
    out.push_str(&names.len().to_string());
    out.push_str("] = [\n");
    for name in &names {
        out.push_str(&format!(
            "    ({name:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \
             \"/assets/tabler/{name}\"))),\n"
        ));
    }
    out.push_str("];\n");
    let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("tabler_assets.rs");
    std::fs::write(path, out).expect("write tabler_assets.rs");
}
