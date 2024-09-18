# jgenesis-proc-macros

Custom derive macros used across the other crates:
* `EnumDisplay`: Generates a `std::fmt::Display` impl for an enum with only fieldless variants
* `EnumFromStr`: Generates a `std::str::FromStr` impl for an enum with only fieldless variants
* `ConfigDisplay`: Generates a `std::fmt::Display` impl meant for pretty-printing config structs
* `FakeEncode` : Generates a `bincode::Encode` impl that does not actually serialize anything, meant for fields such as ROM data and frame buffers
* `FakeDecode`: Generates `bincode::Decode` and `bincode::BorrowDecode` impls that do not actually deserialize anything, meant for fields such as ROM data and frame buffers