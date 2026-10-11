use super::*;

#[test]
fn stages_accumulate_only_while_enabled() {
    take();
    time(Stage::Bubble, || std::thread::sleep(std::time::Duration::from_millis(2)));
    assert_eq!(take()[Stage::Bubble as usize], 0.0);
    enable(true);
    time(Stage::Bubble, || std::thread::sleep(std::time::Duration::from_millis(2)));
    enable(false);
    assert!(take()[Stage::Bubble as usize] >= 2.0);
    assert_eq!(take()[Stage::Bubble as usize], 0.0);
}
