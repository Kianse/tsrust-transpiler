fn main() {
    tsrust_build_driver::compile_project("src").expect("tsrust transpile failed");
    println!(
        "cargo:rustc-env=TSRUST_OUT={}",
        std::env::var("OUT_DIR").unwrap()
    );
}
