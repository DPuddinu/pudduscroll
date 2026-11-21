fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: ui-check <image.png>");
        std::process::exit(2);
    };
    let image = pudduscroll_ui_check::Image::load_png(std::path::Path::new(&path)).expect("png");
    let report = pudduscroll_ui_check::standard().run(&image);
    for finding in &report.findings {
        println!(
            "{}: {} ({}x{} at {},{})",
            finding.check,
            finding.message,
            finding.bounds.w,
            finding.bounds.h,
            finding.bounds.x,
            finding.bounds.y
        );
    }
    println!("{} items, {} findings", report.items, report.findings.len());
    if !report.ok() {
        std::process::exit(1);
    }
}
