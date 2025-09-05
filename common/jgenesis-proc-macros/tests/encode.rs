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

#[derive(FakeEncode, FakeDecode)]
struct HasGenericType<T: Default>(T);

impl<T: Default> Default for HasGenericType<T> {
    fn default() -> Self {
        Self(T::default())
    }
}

#[test]
fn fake_encode_decode_with_generics() {
    let bincode_config = bincode::config::standard();

    let value: HasGenericType<i32> = HasGenericType(42);

    let serialized = bincode::encode_to_vec(value, bincode_config)
        .expect("Failed to serialize using FakeEncode implementation");

    let (deserialized, _) =
        bincode::decode_from_slice::<HasGenericType<i32>, _>(&serialized, bincode_config)
            .expect("Failed to deserialize using FakeDecode implementation");

    assert_eq!(deserialized.0, 0);
}
