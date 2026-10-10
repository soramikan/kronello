//! Surface lifetime bounds shared by DAG admission and concrete backends.

/// Pending GPU cache publications retain at most this many surfaces until
/// sticky validation succeeds. Persistent caches have separate byte budgets.
pub const PENDING_SURFACE_LIMIT: usize = 8;

/// Allocation behavior of one image operation, in execution-input order.
#[derive(Clone, Debug, Default)]
pub struct SurfaceLifetime {
    pub inputs: Vec<usize>,
    /// Composite each child immediately into one accumulator.
    pub streaming: bool,
    /// Extra simultaneously live work surfaces beyond the output and inputs.
    pub scratch: usize,
    /// Input payloads retained outside execution (e.g. decoded raster copies).
    pub resident: usize,
}

/// References include repeated edges and roots. Only multiply referenced
/// results need a pass-local memo; single-use results die in their consumer.
pub fn surface_references(nodes: &[SurfaceLifetime], roots: &[usize]) -> Vec<usize> {
    let mut refs = vec![0; nodes.len()];
    for id in roots.iter().chain(nodes.iter().flat_map(|n| &n.inputs)) {
        refs[*id] += 1;
    }
    refs
}

/// Conservative peak for a validated acyclic graph. Shared results remain in
/// the memo for the pass; siblings in a streaming composite never accumulate.
/// Count full union-ROI surfaces, not geometry bounds: backends allocate these
/// dimensions even for a tiny glyph. Geometry, driver memory and bounded
/// persistent caches are outside this payload estimate.
pub fn peak_surface_count(nodes: &[SurfaceLifetime], roots: &[usize]) -> usize {
    fn peak(id: usize, nodes: &[SurfaceLifetime], memo: &mut [Option<usize>]) -> usize {
        if let Some(value) = memo[id] {
            return value;
        }
        let n = &nodes[id];
        let held = if n.streaming { 1 } else { n.inputs.len() };
        let mut value = 1 + held + n.scratch;
        for (index, input) in n.inputs.iter().enumerate() {
            // CPU allocates its destination before traversing dependencies.
            let held = if n.streaming { 1 } else { 1 + index };
            value = value.max(held + peak(*input, nodes, memo));
        }
        memo[id] = Some(value);
        value
    }
    let refs = surface_references(nodes, roots);
    let retained =
        refs.iter().filter(|r| **r > 1).count() + nodes.iter().map(|n| n.resident).sum::<usize>();
    let mut memo = vec![None; nodes.len()];
    let execution = roots
        .iter()
        .map(|id| 1 + peak(*id, nodes, &mut memo))
        .max()
        .unwrap_or(1);
    // Blank, output conversion/crop/readback, and bounded cache publication.
    retained + execution + 4 + PENDING_SURFACE_LIMIT.min(nodes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_siblings_do_not_accumulate_but_shared_results_do() {
        let graph = |count| {
            let mut nodes = vec![
                SurfaceLifetime {
                    scratch: 1,
                    ..Default::default()
                };
                count
            ];
            nodes.push(SurfaceLifetime {
                inputs: (0..count).collect(),
                streaming: true,
                ..Default::default()
            });
            nodes
        };
        assert_eq!(
            peak_surface_count(&graph(16), &[16]),
            peak_surface_count(&graph(400), &[400])
        );
        let mut shared = graph(400);
        shared[400].inputs.extend(0..400);
        assert_eq!(
            peak_surface_count(&shared, &[400]),
            peak_surface_count(&graph(400), &[400]) + 400
        );
    }
}
