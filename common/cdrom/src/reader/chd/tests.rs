use super::*;

// Based on an image of Ys IV: The Dawn of Ys
const PCE_METADATA: &[&str] = &[
    "TRACK:1 TYPE:AUDIO SUBTYPE:NONE FRAMES:3366 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:2 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:33144 PREGAP:224 PGTYPE:VMODE1_RAW PGSUB:NONE POSTGAP:0\0",
    "TRACK:3 TYPE:AUDIO SUBTYPE:NONE FRAMES:9026 PREGAP:150 PGTYPE:VAUDIO PGSUB:NONE POSTGAP:0\0",
    "TRACK:4 TYPE:AUDIO SUBTYPE:NONE FRAMES:8826 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:5 TYPE:AUDIO SUBTYPE:NONE FRAMES:13272 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:6 TYPE:AUDIO SUBTYPE:NONE FRAMES:12867 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:7 TYPE:AUDIO SUBTYPE:NONE FRAMES:7975 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:8 TYPE:AUDIO SUBTYPE:NONE FRAMES:9098 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:9 TYPE:AUDIO SUBTYPE:NONE FRAMES:8356 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:10 TYPE:AUDIO SUBTYPE:NONE FRAMES:8759 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:11 TYPE:AUDIO SUBTYPE:NONE FRAMES:8566 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:12 TYPE:AUDIO SUBTYPE:NONE FRAMES:8501 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:13 TYPE:AUDIO SUBTYPE:NONE FRAMES:9182 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:14 TYPE:AUDIO SUBTYPE:NONE FRAMES:9253 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:15 TYPE:AUDIO SUBTYPE:NONE FRAMES:9389 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:16 TYPE:AUDIO SUBTYPE:NONE FRAMES:8866 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:17 TYPE:AUDIO SUBTYPE:NONE FRAMES:9495 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:18 TYPE:AUDIO SUBTYPE:NONE FRAMES:9496 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:19 TYPE:AUDIO SUBTYPE:NONE FRAMES:8913 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:20 TYPE:AUDIO SUBTYPE:NONE FRAMES:13851 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:21 TYPE:AUDIO SUBTYPE:NONE FRAMES:7586 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:22 TYPE:AUDIO SUBTYPE:NONE FRAMES:8916 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:23 TYPE:AUDIO SUBTYPE:NONE FRAMES:13395 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:24 TYPE:AUDIO SUBTYPE:NONE FRAMES:6043 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:25 TYPE:AUDIO SUBTYPE:NONE FRAMES:7747 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:26 TYPE:AUDIO SUBTYPE:NONE FRAMES:5639 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:27 TYPE:AUDIO SUBTYPE:NONE FRAMES:4703 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:28 TYPE:AUDIO SUBTYPE:NONE FRAMES:8135 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:29 TYPE:AUDIO SUBTYPE:NONE FRAMES:11918 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:30 TYPE:AUDIO SUBTYPE:NONE FRAMES:23820 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:31 TYPE:AUDIO SUBTYPE:NONE FRAMES:4387 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0",
    "TRACK:32 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:16611 PREGAP:224 PGTYPE:VMODE1_RAW PGSUB:NONE POSTGAP:0\0",
];

const FRAMES: &[u32] = &[
    3366, 33144, 9026, 8826, 13272, 12867, 7975, 9098, 8356, 8759, 8566, 8501, 9182, 9253, 9389,
    8866, 9495, 9496, 8913, 13851, 7586, 8916, 13395, 6043, 7747, 5639, 4703, 8135, 11918, 23820,
    4387, 16611,
];

#[test]
fn pce_metadata() {
    let metadata_parsed: Vec<_> = PCE_METADATA
        .iter()
        .enumerate()
        .map(|(i, metadata_str)| {
            let Some(metadata) = CdMetadata::parse_from(metadata_str.as_bytes().to_vec()) else {
                panic!("Failed to parse track {}: '{metadata_str}'", i + 1);
            };
            metadata
        })
        .collect();

    assert_eq!(
        metadata_parsed[0],
        CdMetadata {
            track_number: 1,
            mode: TrackMode::Audio,
            frames: FRAMES[0],
            pregap_frames: 0,
            pregap_type: Some(PregapType::Mode1),
        },
        "Track 1 parsed"
    );

    assert_eq!(
        metadata_parsed[1],
        CdMetadata {
            track_number: 2,
            mode: TrackMode::Mode1,
            frames: FRAMES[1],
            pregap_frames: 224,
            pregap_type: Some(PregapType::Mode1),
        },
        "Track 2 parsed"
    );

    assert_eq!(
        metadata_parsed[2],
        CdMetadata {
            track_number: 3,
            mode: TrackMode::Audio,
            frames: FRAMES[2],
            pregap_frames: 150,
            pregap_type: Some(PregapType::Audio),
        },
        "Track 3 parsed"
    );

    for track in 4..=31 {
        assert_eq!(
            metadata_parsed[(track - 1) as usize],
            CdMetadata {
                track_number: track,
                mode: TrackMode::Audio,
                frames: FRAMES[(track - 1) as usize],
                pregap_frames: 0,
                pregap_type: Some(PregapType::Mode1),
            },
            "Track {track} parsed"
        );
    }

    assert_eq!(
        metadata_parsed[31],
        CdMetadata {
            track_number: 32,
            mode: TrackMode::Mode1,
            frames: FRAMES[31],
            pregap_frames: 224,
            pregap_type: Some(PregapType::Mode1),
        },
        "Track 32 parsed"
    );
}
