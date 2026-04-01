use std::path::Path;
use std::process::Command;
use std::time::SystemTime;
use std::fs;

const CACHE_TTL_SECS: u64 = 5;

#[derive(Debug, Default)]
pub struct GitInfo {
    pub branch: String,
    pub file_count: usize,
    pub added: usize,
    pub deleted: usize,
}

impl GitInfo {
    pub fn is_dirty(&self) -> bool {
        self.file_count > 0
    }

    pub fn diff_label(&self) -> String {
        if !self.is_dirty() {
            return String::new();
        }
        format!("+{}/-{} ~{}", self.added, self.deleted, self.file_count)
    }
}

/// Get git info for a directory, using a file cache for performance.
pub fn info(dir: &str) -> Option<GitInfo> {
    let cache_path = cache_path_for(dir)?;
    if let Some(cached) = read_cache(&cache_path) {
        return Some(cached);
    }
    let info = collect(dir)?;
    write_cache(&cache_path, &info);
    Some(info)
}

fn collect(dir: &str) -> Option<GitInfo> {
    // Check this is a git repo
    let status = Command::new("git")
        .args(["-C", dir, "rev-parse", "--git-dir"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }

    let branch = Command::new("git")
        .args(["-C", dir, "--no-optional-locks", "branch", "--show-current"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let mut file_count = 0usize;
    let mut added = 0usize;
    let mut deleted = 0usize;

    if let Ok(output) = Command::new("git")
        .args(["-C", dir, "--no-optional-locks", "diff", "HEAD", "--numstat"])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                // Skip binary files (shown as "-")
                if let (Ok(a), Ok(d)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                    file_count += 1;
                    added += a;
                    deleted += d;
                }
            }
        }
    }

    Some(GitInfo {
        branch,
        file_count,
        added,
        deleted,
    })
}

fn cache_path_for(dir: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let cache_dir = Path::new(&home).join(".claude").join("cache");
    fs::create_dir_all(&cache_dir).ok()?;
    // Use a hash of the directory path as cache filename
    let hash = simple_hash(dir);
    Some(cache_dir.join(format!("git-{hash}.cache")))
}

fn simple_hash(s: &str) -> u64 {
    // FNV-1a
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn read_cache(path: &Path) -> Option<GitInfo> {
    let meta = fs::metadata(path).ok()?;
    let age = SystemTime::now()
        .duration_since(meta.modified().ok()?)
        .ok()?
        .as_secs();
    if age >= CACHE_TTL_SECS {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    parse_cache(&content)
}

fn parse_cache(content: &str) -> Option<GitInfo> {
    let parts: Vec<&str> = content.split('\t').collect();
    if parts.len() < 4 {
        return None;
    }
    Some(GitInfo {
        branch: parts[0].to_string(),
        file_count: parts[1].parse().ok()?,
        added: parts[2].parse().ok()?,
        deleted: parts[3].parse().ok()?,
    })
}

fn write_cache(path: &Path, info: &GitInfo) {
    let content = format!("{}\t{}\t{}\t{}", info.branch, info.file_count, info.added, info.deleted);
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, &content).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}
