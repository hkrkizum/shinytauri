use crate::{Result, template};
use std::{
    env,
    path::{self, PathBuf},
    process::Command,
};

fn run(command: &mut Command) -> Result<()> {
    let executable = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .map_err(|err| format!("could not run {executable}: {err}"))?;
    if !status.success() {
        return Err(format!("{executable} failed ({status})").into());
    }
    Ok(())
}

pub(crate) enum Action {
    Build(Option<PathBuf>),
    Lock,
}

pub(crate) fn execute(app: Option<PathBuf>, config: Option<PathBuf>, action: Action) -> Result<()> {
    if !cfg!(all(
        target_arch = "x86_64",
        any(target_os = "linux", windows)
    )) {
        return Err("Building currently requires x86_64 Linux or x64 Windows.".into());
    }
    let config = config.unwrap_or_else(|| {
        app.unwrap_or_else(|| PathBuf::from("."))
            .join("shinytauri.toml")
    });
    let config = path::absolute(config)?;
    if !config.is_file() {
        return Err(format!(
            "No bundle config at {}. Run shinytauri init first, or pass --config.",
            config.display()
        )
        .into());
    }
    let args = match action {
        Action::Build(out) => {
            let out = path::absolute(out.unwrap_or_else(|| config.parent().unwrap().join("dist")))?;
            if out.exists() && !out.is_dir() {
                return Err(format!("Output is not a directory: {}", out.display()).into());
            }
            vec![config.into_os_string(), out.into_os_string()]
        }
        Action::Lock => vec!["--lock".into(), config.into_os_string()],
    };
    let mut scratch = tempfile::Builder::new();
    scratch.prefix("shinytauri-");
    #[cfg(unix)]
    {
        use std::{fs::Permissions, os::unix::fs::PermissionsExt};
        scratch.permissions(Permissions::from_mode(0o700));
    }
    let engine = scratch.tempdir()?;
    template::extract(engine.path())?;
    let context = |program: &str| {
        let mut process = Command::new(program);
        process.current_dir(engine.path()).env("RIG_MODE", "user");
        process
    };
    if cfg!(target_os = "linux")
        && env::var_os("SHINYTAURI_WINDOWS_SHELL").as_deref() != Some("1".as_ref())
    {
        // Only the fixed script is interpreted by sh. The user's paths are positional arguments.
        const BUILD: &str =
            "rig proj sync --locked && exec rig run --rscript scripts/build-windows.R \"$@\"";
        run(context("nix")
            .args([
                "develop",
                "--no-write-lock-file",
                "path:.#windows",
                "-c",
                "sh",
                "-ec",
                BUILD,
                "shinytauri",
            ])
            .args(&args))?;
    } else {
        run(context("rig").args(["proj", "sync", "--locked"]))?;
        run(context("rig")
            .args(["run", "--rscript", "scripts/build-windows.R"])
            .args(&args))?;
    }
    Ok(())
}
