use jgenesis_common::frontend::PartialClone;

#[derive(Debug, PartialEq, PartialClone)]
struct UnitStruct;

#[test]
fn unit_struct() {
    assert_eq!(UnitStruct, UnitStruct.partial_clone());
}

#[derive(Debug, PartialEq, PartialClone)]
struct NestedNamed {
    a: Vec<u8>,
    #[partial_clone(default)]
    b: Vec<u16>,
    c: String,
}

#[derive(Debug, PartialEq, PartialClone)]
struct NamedFields {
    d: Vec<u8>,
    #[partial_clone(default)]
    e: Vec<u16>,
    #[partial_clone(partial)]
    f: NestedNamed,
}

#[test]
fn nested_named_fields() {
    let inner = NestedNamed { a: vec![1, 2, 3], b: vec![4, 5, 6], c: "hello".into() };
    let outer = NamedFields { d: vec![7, 8, 9], e: vec![10, 11, 12], f: inner };

    let expected = NamedFields {
        d: vec![7, 8, 9],
        e: vec![],
        f: NestedNamed { a: vec![1, 2, 3], b: vec![], c: "hello".into() },
    };
    assert_eq!(outer.partial_clone(), expected);
}

#[derive(Debug, PartialEq, PartialClone)]
struct NestedUnnamed(Vec<u8>, #[partial_clone(default)] String);

#[derive(Debug, PartialEq, PartialClone)]
struct GenericUnnamed<T>(#[partial_clone(partial)] T, Vec<u8>);

#[test]
fn nested_unnamed_fields_generic() {
    let inner = NestedUnnamed(vec![1, 2, 3], "hello".into());
    let outer = GenericUnnamed(inner, vec![4, 5, 6]);

    let expected_inner = NestedUnnamed(vec![1, 2, 3], String::new());
    let expected_outer = GenericUnnamed(expected_inner, vec![4, 5, 6]);
    assert_eq!(outer.partial_clone(), expected_outer);
}

#[derive(Debug, PartialEq, PartialClone)]
enum Enum {
    Unit,
    Unnamed(Vec<u8>, #[partial_clone(default)] String, #[partial_clone(partial)] NestedUnnamed),
    Named {
        a: Vec<u8>,
        #[partial_clone(default)]
        b: String,
        #[partial_clone(partial)]
        c: NestedUnnamed,
    },
}

#[test]
fn enum_unit() {
    assert_eq!(Enum::Unit, Enum::Unit.partial_clone());
}

#[test]
fn enum_unnamed() {
    let inner = NestedUnnamed(vec![1, 2, 3], "hello".into());
    let outer = Enum::Unnamed(vec![4, 5, 6], "world".into(), inner);

    let expected_inner = NestedUnnamed(vec![1, 2, 3], String::new());
    let expected_outer = Enum::Unnamed(vec![4, 5, 6], String::new(), expected_inner);
    assert_eq!(outer.partial_clone(), expected_outer);
}

#[test]
fn enum_named() {
    let inner = NestedUnnamed(vec![1, 2, 3], "hello".into());
    let outer = Enum::Named { a: vec![4, 5, 6], b: "world".into(), c: inner };

    let expected_inner = NestedUnnamed(vec![1, 2, 3], String::new());
    let expected_outer = Enum::Named { a: vec![4, 5, 6], b: String::new(), c: expected_inner };
    assert_eq!(outer.partial_clone(), expected_outer);
}
