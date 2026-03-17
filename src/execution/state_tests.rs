use super::state::InstructionPointer;

#[test]
fn new_starts_at_zero() {
    assert_eq!(InstructionPointer::new().path, vec![0]);
}

#[test]
fn depth_matches_path_length() {
    assert_eq!(InstructionPointer::new().depth(), 1);
}

#[test]
fn advance_increments_current_index() {
    let mut ip = InstructionPointer::new();
    ip.advance();
    assert_eq!(ip.path, vec![1]);
}

#[test]
fn descend_appends_zero() {
    let mut ip = InstructionPointer::new();
    ip.advance();
    ip.descend();
    assert_eq!(ip.path, vec![1, 0]);
}

#[test]
fn snapshot_returns_current_path_copy() {
    let mut ip = InstructionPointer::new();
    ip.advance();
    ip.descend();
    ip.advance();
    assert_eq!(ip.snapshot(), vec![1, 1]);
}

#[test]
fn ascend_pops_child_and_advances_parent() {
    let mut ip = InstructionPointer::new();
    ip.advance();
    ip.descend();
    ip.advance();
    ip.ascend();
    assert_eq!(ip.path, vec![2]);
}
