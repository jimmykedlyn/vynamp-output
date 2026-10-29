//! `docs/support-matrix.md` must be exactly what the generator prints.

#[test]
fn committed_matrix_matches_generator() {
    let committed = include_str!("../../../docs/support-matrix.md").replace("\r\n", "\n");
    assert_eq!(
        committed,
        vynamp_output::support_matrix_markdown(),
        "regenerate with: cargo run -p vynamp-output --example support_matrix > docs/support-matrix.md"
    );
}
