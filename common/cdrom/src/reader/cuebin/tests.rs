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
            file_type: FileType::Binary,
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
            file_type: FileType::Binary,
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
            file_type: FileType::Binary,
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
                file_type: FileType::Binary,
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

// Based on an image of Ys IV: The Dawn of Ys
const PCE_SAMPLE: &str = r#"
FILE "Test (Track 01).bin" BINARY
  TRACK 01 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 02).bin" BINARY
  TRACK 02 MODE1/2352
    INDEX 00 00:00:00
    INDEX 01 00:02:74
FILE "Test (Track 03).bin" BINARY
  TRACK 03 AUDIO
    INDEX 00 00:00:00
    INDEX 01 00:02:00
FILE "Test (Track 04).bin" BINARY
  TRACK 04 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 05).bin" BINARY
  TRACK 05 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 06).bin" BINARY
  TRACK 06 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 07).bin" BINARY
  TRACK 07 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 08).bin" BINARY
  TRACK 08 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 09).bin" BINARY
  TRACK 09 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 10).bin" BINARY
  TRACK 10 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 11).bin" BINARY
  TRACK 11 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 12).bin" BINARY
  TRACK 12 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 13).bin" BINARY
  TRACK 13 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 14).bin" BINARY
  TRACK 14 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 15).bin" BINARY
  TRACK 15 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 16).bin" BINARY
  TRACK 16 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 17).bin" BINARY
  TRACK 17 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 18).bin" BINARY
  TRACK 18 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 19).bin" BINARY
  TRACK 19 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 20).bin" BINARY
  TRACK 20 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 21).bin" BINARY
  TRACK 21 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 22).bin" BINARY
  TRACK 22 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 23).bin" BINARY
  TRACK 23 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 24).bin" BINARY
  TRACK 24 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 25).bin" BINARY
  TRACK 25 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 26).bin" BINARY
  TRACK 26 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 27).bin" BINARY
  TRACK 27 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 28).bin" BINARY
  TRACK 28 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 29).bin" BINARY
  TRACK 29 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 30).bin" BINARY
  TRACK 30 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 31).bin" BINARY
  TRACK 31 AUDIO
    INDEX 01 00:00:00
FILE "Test (Track 32).bin" BINARY
  TRACK 32 MODE1/2352
    INDEX 00 00:00:00
    INDEX 01 00:02:74
"#;

#[test]
fn pce_cue() {
    let files = CueParser::new().parse(PCE_SAMPLE).unwrap();
    assert_eq!(files.len(), 32);

    assert_eq!(
        files[0],
        ParsedFile {
            file_name: "Test (Track 01).bin".into(),
            file_type: FileType::Binary,
            tracks: vec![ParsedTrack {
                number: 1,
                mode: TrackMode::Audio,
                pregap_len: None,
                pause_start: None,
                track_start: CdTime::ZERO,
            }]
        },
        "Track 1 parsed"
    );

    assert_eq!(
        files[1],
        ParsedFile {
            file_name: "Test (Track 02).bin".into(),
            file_type: FileType::Binary,
            tracks: vec![ParsedTrack {
                number: 2,
                mode: TrackMode::Mode1,
                pregap_len: None,
                pause_start: Some(CdTime::ZERO),
                track_start: CdTime::new(0, 2, 74),
            }],
        },
        "Track 2 parsed"
    );

    assert_eq!(
        files[2],
        ParsedFile {
            file_name: "Test (Track 03).bin".into(),
            file_type: FileType::Binary,
            tracks: vec![ParsedTrack {
                number: 3,
                mode: TrackMode::Audio,
                pregap_len: None,
                pause_start: Some(CdTime::ZERO),
                track_start: CdTime::new(0, 2, 0),
            }]
        },
        "Track 3 parsed"
    );

    for track in 4..=31 {
        assert_eq!(
            files[(track - 1) as usize],
            ParsedFile {
                file_name: format!("Test (Track {track:02}).bin"),
                file_type: FileType::Binary,
                tracks: vec![ParsedTrack {
                    number: track,
                    mode: TrackMode::Audio,
                    pregap_len: None,
                    pause_start: None,
                    track_start: CdTime::ZERO,
                }],
            },
            "Track {track} parsed"
        );
    }

    assert_eq!(
        files[31],
        ParsedFile {
            file_name: "Test (Track 32).bin".into(),
            file_type: FileType::Binary,
            tracks: vec![ParsedTrack {
                number: 32,
                mode: TrackMode::Mode1,
                pregap_len: None,
                pause_start: Some(CdTime::ZERO),
                track_start: CdTime::new(0, 2, 74),
            }]
        },
        "Track 32 parsed"
    );
}
