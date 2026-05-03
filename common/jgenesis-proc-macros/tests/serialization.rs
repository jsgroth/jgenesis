use jgenesis_proc_macros::deserialize_default_on_error;
use serde::Deserialize;

#[deserialize_default_on_error]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Foo {
    a: u32,
    b: u32,
}

impl Default for Foo {
    fn default() -> Self {
        Self { a: 5, b: 10 }
    }
}

#[test_log::test]
fn test_deserialize_default_on_error() {
    assert_eq!(toml::from_str::<Foo>(""), Ok(Foo::default()));

    assert_eq!(toml::from_str::<Foo>("a = 20"), Ok(Foo { a: 20, b: 10 }));

    assert_eq!(toml::from_str::<Foo>("b = 20"), Ok(Foo { a: 5, b: 20 }));

    assert_eq!(toml::from_str::<Foo>("a = \"asdf\""), Ok(Foo { a: 5, b: 10 }));

    assert_eq!(toml::from_str::<Foo>("a = 20\nb = \"asdf\""), Ok(Foo { a: 20, b: 10 }));

    assert_eq!(toml::from_str::<Foo>("a = 20\nb = 30"), Ok(Foo { a: 20, b: 30 }));
}
