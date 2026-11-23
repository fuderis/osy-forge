use crate::prelude::*;

#[derive(Debug, Clone)]
pub struct CrateVersionInfo {
    /// Display name, e.g. `kernel (crates/kernel)`.
    pub name: String,
    /// Relative path to Cargo.toml.
    pub manifest_path: PathBuf,
    pub version: String,
}

#[derive(Debug, Clone)]
pub enum VersionResolution {
    Single(CrateVersionInfo),
    Multiple(Vec<CrateVersionInfo>),
    NoneFound,
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Parses `[package].name` and `[package].version` from a Cargo.toml file.
fn parse_cargo_package_version(content: &str) -> (Option<String>, Option<String>) {
    let mut in_package_section = false;
    let mut pkg_name: Option<String> = None;
    let mut pkg_version: Option<String> = None;

    for raw_line in content.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            let section = line[1..line.len() - 1].trim();
            in_package_section = section == "package";
            continue;
        }

        if !in_package_section {
            continue;
        }

        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim();
            let val = val.trim().trim_matches('"').trim_matches('\'').to_string();
            if val.is_empty() {
                continue;
            }
            match key {
                "name" if pkg_name.is_none() => pkg_name = Some(val),
                "version" if pkg_version.is_none() => pkg_version = Some(val),
                _ => {}
            }
        }
    }

    (pkg_name, pkg_version)
}

fn build_crate_info(rel_manifest: PathBuf, content: &str) -> Option<CrateVersionInfo> {
    let (pkg_name, Some(version)) = parse_cargo_package_version(content) else {
        return None;
    };

    let rel_dir = rel_manifest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| rel_manifest.display().to_string());

    let display_name = match pkg_name {
        Some(n) => format!("{n} ({rel_dir})"),
        None => rel_dir,
    };

    Some(CrateVersionInfo {
        name: display_name,
        manifest_path: rel_manifest,
        version,
    })
}

/// Resolves Cargo package version from client `Cargo.toml`, or scans workspace crates if root has no version.
pub async fn resolve_cargo_version(
    tx: &Sender<Bytes>,
    repo_path: &Path,
) -> Result<VersionResolution> {
    let dir_q = shell_quote(&repo_path.to_string_lossy());
    let root_cmd =
        format!("if [ -f {dir_q}/Cargo.toml ]; then cat {dir_q}/Cargo.toml; else exit 1; fi");

    let root_res = match Dialog::script(tx, root_cmd, Duration::from_secs(60)).await? {
        Some(res) => res,
        None => return Err(Error::OperationTimeout.into()),
    };

    if !root_res.success {
        return Ok(VersionResolution::NoneFound);
    }

    let (pkg_name, version) = parse_cargo_package_version(&root_res.stdout);
    if let Some(ver) = version {
        let name = pkg_name.unwrap_or_else(|| "root".to_string());
        return Ok(VersionResolution::Single(CrateVersionInfo {
            name,
            manifest_path: PathBuf::from("Cargo.toml"),
            version: ver,
        }));
    }

    let scan_cmd = format!(
        "cd {dir_q} && find . -name Cargo.toml -not -path './Cargo.toml' -not -path '*/target/*' -not -path '*/.git/*' -not -path '*/node_modules/*' | while IFS= read -r f; do printf '===MANIFEST:%s===\\n' \"$f\" && cat \"$f\"; done"
    );

    let scan_res = match Dialog::script(tx, scan_cmd, Duration::from_secs(60)).await? {
        Some(res) => res,
        None => return Err(Error::OperationTimeout.into()),
    };

    if !scan_res.success {
        return Ok(VersionResolution::NoneFound);
    }

    let mut members = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_content = String::new();

    for line in scan_res.stdout.lines() {
        if let Some(rest) = line.strip_prefix("===MANIFEST:") {
            if let Some(path_str) = rest.strip_suffix("===") {
                if let Some(prev_path) = current_path.take() {
                    if let Some(info) = build_crate_info(prev_path, &current_content) {
                        members.push(info);
                    }
                    current_content.clear();
                }
                let clean = path_str.strip_prefix("./").unwrap_or(path_str);
                current_path = Some(PathBuf::from(clean));
                continue;
            }
        }
        current_content.push_str(line);
        current_content.push('\n');
    }

    if let Some(last_path) = current_path {
        if let Some(info) = build_crate_info(last_path, &current_content) {
            members.push(info);
        }
    }

    members.sort_by(|a, b| a.name.cmp(&b.name));

    match members.len() {
        0 => Ok(VersionResolution::NoneFound),
        1 => Ok(VersionResolution::Single(members.remove(0))),
        _ => Ok(VersionResolution::Multiple(members)),
    }
}
