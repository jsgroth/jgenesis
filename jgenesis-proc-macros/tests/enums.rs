use jgenesis_proc_macros::EnumAll;

#[derive(Debug, PartialEq, EnumAll)]
enum Unit {}

#[test]
fn enum_all_unit() {
    assert_eq!(Unit::ALL, []);
}
