use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[must_use]
pub fn is_m3u(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("m3u"))
}

/// # Errors
///
/// Returns an error if there is an I/O error reading the file or if the M3U playlist is invalid.
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

    #[test]
    fn parses_relative_paths_and_ignores_comments_and_blank_lines() {
        let temp_dir = std::env::temp_dir().join("jgenesis_m3u_test_relative");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let playlist_path = temp_dir.join("game.m3u");
        let disc_1_path: String =
            Path::new("Disc 1").join("Game (Disc 1).cue").to_string_lossy().into();
        let disc_2_path: String =
            Path::new("Disc 2").join("Game (Disc 2).cue").to_string_lossy().into();
        fs::write(&playlist_path, &format!("#EXTM3U\n\n{disc_1_path}\n# Comment\n{disc_2_path}\n"))
            .unwrap();

        let paths = parse(&playlist_path).unwrap();

        assert_eq!(paths, vec![temp_dir.join(disc_1_path), temp_dir.join(disc_2_path)]);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn strips_utf8_bom() {
        let temp_dir = std::env::temp_dir().join("jgenesis_m3u_test_bom");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let playlist_path = temp_dir.join("game.m3u");
        let disc_path: String = Path::new("Disc 1").join("Game.cue").to_string_lossy().into();
        fs::write(&playlist_path, format!("\u{feff}{disc_path}\r\n")).unwrap();

        let paths = parse(&playlist_path).unwrap();

        assert_eq!(paths, vec![temp_dir.join(disc_path)]);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn rejects_empty_playlist() {
        let temp_dir = std::env::temp_dir().join("jgenesis_m3u_test_empty");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let playlist_path = temp_dir.join("game.m3u");
        fs::write(&playlist_path, "#EXTM3U\n\n# No discs\n").unwrap();

        let err = parse(&playlist_path).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
