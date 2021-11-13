use example::{from_str, Character};

#[test]
fn test_load_from_str() {
    let ch: Character = from_str(
        r#"{
        "name": "Luke Skywalker",
        "appears_in": [4, 5, 6],
        "friends": [{"name": "Han Solo"}]
    }"#,
    )
    .unwrap();
    assert_eq!(ch.name, "Luke Skywalker");
}
