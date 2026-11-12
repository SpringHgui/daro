fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icons/ui/logo.ico");
    let _ = embed_resource::compile("assets/app.rc", embed_resource::NONE);
}
