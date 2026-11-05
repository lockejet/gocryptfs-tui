// Web UI 入口（开发中）。当前仅提供占位的 --help / --version。
use gocryptfs_ui_core::{version_string, PROJECT_NAME};

fn usage() {
    println!("gocryptfs-web — Web UI for gocryptfs (work in progress)");
    println!("Not implemented yet; coming soon.");
    println!("Part of the {PROJECT_NAME} project.");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        usage();
        return;
    }

    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("gocryptfs-web {}", version_string());
        return;
    }

    eprintln!("Not implemented yet; coming soon.");
    std::process::exit(1);
}
