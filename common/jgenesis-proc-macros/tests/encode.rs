use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::fs::File;

#[derive(FakeEncode, FakeDecode)]
struct NotSerializable {
    file: Option<File>,
}

impl Default for NotSerializable {
    fn default() -> Self {
        Self { file: None }
    }
}

#[test]
fn fake_encode_decode() {
    let bincode_config = bincode::config::standard();

    let not_serializable = NotSerializable { file: None };

    let serialized = bincode::encode_to_vec(not_serializable, bincode_config)
        .expect("Failed to serialize using FakeEncode implementation");

    let (deserialized, _) =
        bincode::decode_from_slice::<NotSerializable, _>(&serialized, bincode_config)
            .expect("Failed to deserialize using FakeDecode implementation");
    assert!(deserialized.file.is_none());
}
