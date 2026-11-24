#[test]
fn product_value() {
    assert_eq!(product::value(), 41);
}

#[test]
fn product_twice() {
    assert_eq!(product::value() * 2, 82);
}
