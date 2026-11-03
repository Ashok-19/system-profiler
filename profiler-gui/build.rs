use slint_build::CompilerConfiguration;

fn main() {
    // Every control is drawn by the app itself; the style only affects the
    // few std-widgets used (scrollbars).
    let config = CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("compile Slint UI");
}
