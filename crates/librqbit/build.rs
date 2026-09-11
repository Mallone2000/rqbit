use anyhow::{Context, bail};
use std::path::Path;
use std::process::Command;

#[allow(dead_code)]
fn run_npm(cwd: &Path, args: &[&str]) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    let npm = "npm.cmd";
    #[cfg(not(target_os = "windows"))]
    let npm = "npm";
    let command = format!("npm {}", args.join(" "));

    let output = Command::new(npm)
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| {
            format!(
                "Failed to execute {} in {:?}. PATH: {:?}",
                command,
                cwd,
                std::env::var("PATH").unwrap_or_default()
            )
        })?;

    if !output.status.success() {
        bail!(
            "\"{}\" failed\n\nstderr: {}\n\nstdout: {}",
            command,
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }

    // Optionally print the stdout output if you want to see the build logs
    println!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}

fn main() {
    #[cfg(feature = "webui")]
    {
        let webui_dir = Path::new("webui");
        let webui_src_dir = webui_dir.join("src");

        println!("cargo:rerun-if-changed={}", webui_src_dir.to_str().unwrap());

        // Use the lockfile and invoke npm directly without a command shell.
        for args in [["ci"].as_slice(), ["run", "build"].as_slice()] {
            run_npm(webui_dir, args).unwrap();
        }
    }
}
