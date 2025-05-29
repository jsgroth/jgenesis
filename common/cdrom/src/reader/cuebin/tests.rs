use super::*;

const STANDARD_SPACE: &str = "
FILE \"Standard Space.bin\" BINARY
  TRACK 01 MODE1/2352
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    PREGAP 00:02:00
    INDEX 01 13:10:11
  TRACK 03 AUDIO
    INDEX 00 13:14:25
    INDEX 01 13:16:25
";

#[test]
fn single_file_standard_space() {
    let files = CueParser::new().parse(STANDARD_SPACE).unwrap();
    assert_eq!(
        files,
        vec![ParsedFile {
            file_name: "Standard Space.bin".into(),
            tracks: vec![
                ParsedTrack {
                    number: 1,
                    mode: TrackMode::Mode1,
                    pregap_len: None,
                    pause_start: None,
                    track_start: CdTime::new(0, 0, 0),
                },
                ParsedTrack {
                    number: 2,
                    mode: TrackMode::Audio,
                    pregap_len: Some(CdTime::new(0, 2, 0)),
                    pause_start: None,
                    track_start: CdTime::new(13, 10, 11),
                },
                ParsedTrack {
                    number: 3,
                    mode: TrackMode::Audio,
                    pregap_len: None,
                    pause_start: Some(CdTime::new(13, 14, 25)),
                    track_start: CdTime::new(13, 16, 25),
                }
            ]
        }]
    )
}

const MORE_SPACE: &str = "
FILE \"More Space.bin\" BINARY
    TRACK 01 MODE1/2352
      INDEX 01 00:00:00
    TRACK 02 AUDIO
      INDEX 00 01:31:14
      INDEX 01 01:33:14
    TRACK 03 AUDIO
      INDEX 00 01:38:14
      INDEX 01 01:40:14
";

#[test]
fn single_file_more_space() {
    let files = CueParser::new().parse(MORE_SPACE).unwrap();
    assert_eq!(
        files,
        vec![ParsedFile {
            file_name: "More Space.bin".into(),
            tracks: vec![
                ParsedTrack {
                    number: 1,
                    mode: TrackMode::Mode1,
                    pregap_len: None,
                    pause_start: None,
                    track_start: CdTime::new(0, 0, 0),
                },
                ParsedTrack {
                    number: 2,
                    mode: TrackMode::Audio,
                    pregap_len: None,
                    pause_start: Some(CdTime::new(1, 31, 14)),
                    track_start: CdTime::new(1, 33, 14),
                },
                ParsedTrack {
                    number: 3,
                    mode: TrackMode::Audio,
                    pregap_len: None,
                    pause_start: Some(CdTime::new(1, 38, 14)),
                    track_start: CdTime::new(1, 40, 14),
                }
            ]
        }]
    )
}

const MULTI_FILE: &str = "
FILE \"Multi File (Track 01).bin\" BINARY
  TRACK 01 MODE1/2352
    INDEX 01 00:00:00
FILE \"Multi File (Track 02).bin\" BINARY
  TRACK 02 AUDIO
    INDEX 00 00:00:00
    INDEX 01 00:02:00
FILE \"Multi File (Track 03).bin\" BINARY
  TRACK 03 AUDIO
    INDEX 00 00:00:00
    INDEX 01 00:02:00
";

#[test]
fn multi_file() {
    let files = CueParser::new().parse(MULTI_FILE).unwrap();
    assert_eq!(files.len(), 3);

    assert_eq!(
        files[0],
        ParsedFile {
            file_name: "Multi File (Track 01).bin".into(),
            tracks: vec![ParsedTrack {
                number: 1,
                mode: TrackMode::Mode1,
                pregap_len: None,
                pause_start: None,
                track_start: CdTime::new(0, 0, 0),
            }]
        }
    );

    for i in [1, 2] {
        let track_num = i + 1;
        let file_name = format!("Multi File (Track {track_num:02}).bin");
        assert_eq!(
            files[i],
            ParsedFile {
                file_name,
                tracks: vec![ParsedTrack {
                    number: track_num as u8,
                    mode: TrackMode::Audio,
                    pregap_len: None,
                    pause_start: Some(CdTime::new(0, 0, 0)),
                    track_start: CdTime::new(0, 2, 0),
                }]
            }
        );
    }
}
