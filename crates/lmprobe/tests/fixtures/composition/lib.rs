// Tiny composition-demo fixture for Phase 4E-B smoke goldens.
// Two named callers of `compute_total` so callers + enclosing_decl
// have a deterministic shape.

pub fn compute_total(items: &[u32]) -> u32 {
    items.iter().sum()
}

pub fn one_caller() -> u32 {
    compute_total(&[1, 2, 3])
}

pub fn another_caller(values: &[u32]) -> u32 {
    let total = compute_total(values);
    total * 2
}
