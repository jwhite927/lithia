#[allow(dead_code)]
use lithia::util::Password;

#[test]
fn password_visible() {
    let mut pass = Password::from("hello");
    pass.toggle();
    assert_eq!(format!("{}", pass), "hello");
}

#[test]
fn password_hidden() {
    let pass = Password::from("world!");
    assert_eq!(format!("{}", pass), "●●●●●●");
}
