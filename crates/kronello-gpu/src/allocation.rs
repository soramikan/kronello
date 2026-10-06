//! Concrete descriptor payloads held by tracked renderer resources. Driver
//! private allocation, alignment, compression and native codec pools are unknown.
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResourceKind {
    Graph,
    Control,
    Readback,
    Resident,
    Copy,
}
impl ResourceKind {
    fn index(self) -> usize {
        self as usize
    }
}
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct GpuResourceUsage {
    pub live_payload_bytes: u64,
    pub peak_payload_bytes: u64,
    pub live_resources: u64,
    pub peak_resources: u64,
    pub acquisitions: u64,
}
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct GpuNodeResourcePeak {
    pub node: usize,
    pub peak_owned_payload_bytes: u64,
}
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct GpuAllocationStats {
    pub graph_and_cache_surfaces: GpuResourceUsage,
    pub control_buffers: GpuResourceUsage,
    pub readback_buffers: GpuResourceUsage,
    pub resident_inputs: GpuResourceUsage,
    pub output_copies: GpuResourceUsage,
    pub live_owned_payload_bytes: u64,
    pub peak_owned_payload_bytes: u64,
    pub idle_pool_payload_bytes: u64,
    pub node_peaks: Vec<GpuNodeResourcePeak>,
    pub driver_private_bytes_known: bool,
    pub native_decoder_pool_bytes_known: bool,
}
#[derive(Default)]
pub(crate) struct AllocationTracker {
    usage: [GpuResourceUsage; 5],
    live: u64,
    peak: u64,
    sequence: u64,
    scopes: Vec<(u64, usize, u64)>,
    nodes: Vec<GpuNodeResourcePeak>,
}
#[derive(Debug, Clone)]
pub(crate) struct AllocationGuard {
    _inner: Arc<GuardInner>,
}
struct GuardInner {
    tracker: Arc<Mutex<AllocationTracker>>,
    kind: ResourceKind,
    bytes: u64,
}
impl std::fmt::Debug for GuardInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AllocationGuard")
            .field("bytes", &self.bytes)
            .finish()
    }
}
impl Drop for GuardInner {
    fn drop(&mut self) {
        let mut tracker = self.tracker.lock().unwrap_or_else(|e| e.into_inner());
        tracker.live -= self.bytes;
        let usage = &mut tracker.usage[self.kind.index()];
        usage.live_payload_bytes -= self.bytes;
        usage.live_resources -= 1;
    }
}
impl AllocationGuard {
    pub(crate) fn new(
        tracker: &Arc<Mutex<AllocationTracker>>,
        kind: ResourceKind,
        bytes: u64,
    ) -> Self {
        let mut state = tracker.lock().unwrap_or_else(|e| e.into_inner());
        state.live += bytes;
        state.peak = state.peak.max(state.live);
        let live = state.live;
        for (_, _, peak) in &mut state.scopes {
            *peak = (*peak).max(live);
        }
        let usage = &mut state.usage[kind.index()];
        usage.live_payload_bytes += bytes;
        usage.live_resources += 1;
        usage.peak_payload_bytes = usage.peak_payload_bytes.max(usage.live_payload_bytes);
        usage.peak_resources = usage.peak_resources.max(usage.live_resources);
        usage.acquisitions += 1;
        drop(state);
        Self {
            _inner: Arc::new(GuardInner {
                tracker: tracker.clone(),
                kind,
                bytes,
            }),
        }
    }
}
pub(crate) struct NodeScope {
    tracker: Arc<Mutex<AllocationTracker>>,
    token: u64,
}
impl Drop for NodeScope {
    fn drop(&mut self) {
        let mut state = self.tracker.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = state
            .scopes
            .iter()
            .position(|(token, _, _)| *token == self.token)
        {
            let (_, node, peak_owned_payload_bytes) = state.scopes.remove(index);
            if state.nodes.len() == 4096 {
                state.nodes.remove(0);
            }
            state.nodes.push(GpuNodeResourcePeak {
                node,
                peak_owned_payload_bytes,
            });
        }
    }
}
impl crate::GpuContext {
    pub(crate) fn track_resource(&self, kind: ResourceKind, bytes: u64) -> AllocationGuard {
        AllocationGuard::new(&self.allocations, kind, bytes)
    }
    pub(crate) fn track_node(&self, node: usize) -> NodeScope {
        let mut state = self.allocations.lock().unwrap_or_else(|e| e.into_inner());
        state.sequence += 1;
        let token = state.sequence;
        let live = state.live;
        state.scopes.push((token, node, live));
        NodeScope {
            tracker: self.allocations.clone(),
            token,
        }
    }
    /// Reset observations while preserving the live ownership ledger. Use only
    /// between serialized render calls, after requested completion fences.
    pub fn reset_allocation_peaks(&self) -> Result<(), crate::GpuError> {
        let _scope = self.render_scope()?;
        let mut state = self.allocations.lock().unwrap_or_else(|e| e.into_inner());
        if !state.scopes.is_empty() {
            return Err(crate::GpuError::InvalidInput(
                "cannot reset active GPU node observations",
            ));
        }
        state.peak = state.live;
        state.nodes.clear();
        for usage in &mut state.usage {
            usage.peak_payload_bytes = usage.live_payload_bytes;
            usage.peak_resources = usage.live_resources;
            usage.acquisitions = 0;
        }
        Ok(())
    }
    pub fn allocation_stats(&self) -> GpuAllocationStats {
        let state = self.allocations.lock().unwrap_or_else(|e| e.into_inner());
        let result = GpuAllocationStats {
            graph_and_cache_surfaces: state.usage[0],
            control_buffers: state.usage[1],
            readback_buffers: state.usage[2],
            resident_inputs: state.usage[3],
            output_copies: state.usage[4],
            live_owned_payload_bytes: state.live,
            peak_owned_payload_bytes: state.peak,
            node_peaks: state.nodes.clone(),
            ..GpuAllocationStats::default()
        };
        drop(state);
        GpuAllocationStats {
            idle_pool_payload_bytes: self.cache_stats().pool.bytes as u64,
            ..result
        }
    }
}
