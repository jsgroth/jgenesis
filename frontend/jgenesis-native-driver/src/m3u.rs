use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn is_m3u(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("m3u"))
}

pub fn parse(path: &Path) -> io::Result<Vec<PathBuf>> {
    let contents = fs::read_to_string(path)?;
    let parent = path.parent().unwrap_or_else(|| Path::new(""));

    let paths = contents
        .lines()
        .map(str::trim)
        .map(|line| line.trim_start_matches('\u{feff}'))
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let disc_path = PathBuf::from(line);

            if disc_path.is_absolute() { disc_path } else { parent.join(disc_path) }
        })
        .collect::<Vec<_>>();

    if paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "M3U playlist contains no disc entries",
        ));
    }

    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_m3u_extension_case_insensitively() {
        assert!(is_m3u(Path::new("game.m3u")));
        assert!(is_m3u(Path::new("game.M3U")));
        assert!(!is_m3u(Path::new("game.cue")));
    }
}
