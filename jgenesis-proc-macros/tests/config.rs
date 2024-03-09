use jgenesis_proc_macros::ConfigDisplay;

#[derive(ConfigDisplay)]
struct Config<T> {
    field: T,
}

#[test]
fn config_display_generic() {
    let config = Config { field: String::from("hello") };
    let s = format!("config: {config}");
    assert_eq!(s, "config: \n  field: hello");
}
