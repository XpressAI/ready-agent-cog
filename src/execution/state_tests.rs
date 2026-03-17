use super::state::InstructionPointer;

#[test]
fn instruction_pointer_navigation_and_snapshot_work() {
    let mut ip = InstructionPointer::new();
    assert_eq!(ip.path, vec![0]);
    assert_eq!(ip.depth(), 1);

    ip.advance();
    assert_eq!(ip.path, vec![1]);

    ip.descend();
    assert_eq!(ip.path, vec![1, 0]);
    assert_eq!(ip.depth(), 2);

    ip.advance();
    assert_eq!(ip.snapshot(), vec![1, 1]);

    ip.ascend();
    assert_eq!(ip.path, vec![2]);
    assert_eq!(ip.depth(), 1);
}
