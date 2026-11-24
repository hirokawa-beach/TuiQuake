use std::{
    env,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    // Archives built outside a checkout have no Git metadata.
    if Path::new(".git").exists() {
        for reference in [
            Some("HEAD".into()),
            Some("packed-refs".into()),
            git(&["symbolic-ref", "-q", "HEAD"]),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(path) = git(&["rev-parse", "--git-path", &reference])
                && Path::new(&path).exists()
            {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
    let hash = Path::new(".git")
        .exists()
        .then(|| git(&["rev-parse", "--short=12", "HEAD"]))
        .flatten()
        .unwrap_or_else(|| "不明".into());
    let epoch = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });
    println!("cargo:rustc-env=TUIQUAKE_BUILD_COMMIT={hash}");
    println!("cargo:rustc-env=TUIQUAKE_BUILD_EPOCH={epoch}");
}
