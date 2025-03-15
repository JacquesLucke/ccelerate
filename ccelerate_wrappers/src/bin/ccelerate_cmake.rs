use std::io::Write;

fn main() {
    let Ok(current_exe) = std::env::current_exe() else {
        eprintln!("Failed to get current exe");
        std::process::exit(1);
    };
    let Some(current_dir) = current_exe.parent() else {
        eprintln!("Failed to get current dir");
        std::process::exit(1);
    };
    let ccelerate_ar_path = current_dir.join("ccelerate_ar");
    let ccelerate_gcc_path = current_dir.join("ccelerate_gcc");
    let ccelerate_gxx_path = current_dir.join("ccelerate_gxx");

    print!(
        "ccelerate overrides:\n  {}\n  {}\n  {}\n\n",
        ccelerate_ar_path.display(),
        ccelerate_gcc_path.display(),
        ccelerate_gxx_path.display()
    );

    let child = std::process::Command::new("cmake")
        .args(std::env::args_os().skip(1))
        .arg(format!("-DCMAKE_AR={}", ccelerate_ar_path.display()))
        .env("CC", ccelerate_gcc_path)
        .env("CXX", ccelerate_gxx_path)
        .spawn();
    let Ok(child) = child else {
        eprintln!("Failed to spawn cmake");
        std::process::exit(1);
    };
    let Ok(result) = child.wait_with_output() else {
        eprintln!("Failed to wait on cmake");
        std::process::exit(1);
    };
    std::io::stdout().write_all(&result.stdout).unwrap();
    std::io::stderr().write_all(&result.stderr).unwrap();
    std::process::exit(result.status.code().unwrap_or(1));
}
