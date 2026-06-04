#[test]
fn adds_then_doubles() {
    // modules are exposed as hello_tsrust::<file_stem>::
    assert_eq!(hello_tsrust::math::add(2, 3), 5);
    assert_eq!(hello_tsrust::util::double(7), 14);
    assert_eq!(hello_tsrust::main::add_then_double(2, 3), 10);
}
