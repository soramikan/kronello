//! Deterministic forward source-time particle simulation. Cache entries are
//! disposable accelerators; immutable configuration and inputs define state.
use kronello_model::{CompositionInstanceId, ContentId, InstancePath};
use kronello_time::{Duration, Time, TimeError};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const SIMULATION_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulationConfig {
    pub emitter: ContentId,
    pub instance: InstancePath,
    pub seed: u64,
    pub start: Time,
    pub step: Duration,
    pub lifetime: Duration,
    pub emission_interval: Duration,
    pub checkpoint_stride: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParticleInputs {
    pub origin: [f64; 2],
    pub velocity: [f64; 2],
    pub jitter: [f64; 2],
    pub acceleration: [f64; 2],
    pub enabled: bool,
    pub birth_count: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Particle {
    pub id: CompositionInstanceId,
    pub birth: Time,
    pub position: [f64; 2],
    pub velocity: [f64; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimulationState {
    pub tick: u64,
    pub next_birth_serial: u64,
    pub particles: Vec<Particle>,
}

#[derive(Clone, Copy, Debug)]
pub struct SimulationLimits {
    pub max_steps: u64,
    pub max_particles: usize,
    pub max_particle_updates: u64,
    pub max_births: u64,
    pub max_checkpoints: usize,
    pub max_cached_particles: usize,
}
impl Default for SimulationLimits {
    fn default() -> Self {
        Self {
            max_steps: 100_000,
            max_particles: 10_000,
            max_particle_updates: 1_000_000,
            max_births: 100_000,
            max_checkpoints: 32,
            max_cached_particles: 100_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimulationStats {
    pub replayed_steps: u64,
    pub sampled_inputs: u64,
    pub particle_updates: u64,
    pub births: u64,
    pub checkpoint_hit: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SimulationError<E> {
    #[error("invalid simulation configuration")]
    InvalidConfig,
    #[error("invalid or non-finite dynamics input")]
    InvalidInput,
    #[error("simulation budget exceeded")]
    BudgetExceeded,
    #[error("simulation time arithmetic failed: {0}")]
    Time(#[from] TimeError),
    #[error("simulation input evaluation failed")]
    Input(E),
}

#[derive(Debug)]
pub struct SimulationCache {
    limits: SimulationLimits,
    identity: Option<[u8; 32]>,
    checkpoints: BTreeMap<u64, SimulationState>,
}
impl SimulationCache {
    pub fn new(limits: SimulationLimits) -> Self {
        Self {
            limits,
            identity: None,
            checkpoints: BTreeMap::new(),
        }
    }
    pub fn clear(&mut self) {
        self.identity = None;
        self.checkpoints.clear();
    }
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }
    pub fn cached_particle_count(&self) -> usize {
        self.checkpoints.values().map(|s| s.particles.len()).sum()
    }
    /// Return the state at the greatest fixed source tick not after `source_time`.
    /// Before `start`, return an empty state without sampling dynamics.
    ///
    /// The caller must supply an immutable transitive dynamics hash and a pure
    /// callback in the canonical source clock. Equal hashes promise equal input
    /// values at every tick; appearance-only changes may reuse this cache.
    /// Limits bound work for this request, not historical cached work. A warm
    /// request may succeed where a cold replay exhausts its work budget.
    /// Errors never publish a partially updated tick as a checkpoint.
    pub fn state_at<E>(
        &mut self,
        config: &SimulationConfig,
        dynamics_hash: [u8; 32],
        source_time: Time,
        mut inputs: impl FnMut(Time) -> Result<ParticleInputs, E>,
    ) -> Result<(SimulationState, SimulationStats), SimulationError<E>> {
        let stride = validate(config)?;
        let identity = identity(config, dynamics_hash);
        if self.identity != Some(identity) {
            self.checkpoints.clear();
            self.identity = Some(identity);
        }
        let mut stats = SimulationStats::default();
        if source_time < config.start {
            return Ok((empty(), stats));
        }
        let target = source_time
            .checked_sub(config.start)?
            .checked_div(config.step.as_time())?
            .floor();
        let target = u64::try_from(target).map_err(|_| SimulationError::InvalidConfig)?;
        let mut state = if let Some((_, state)) = self.checkpoints.range(..=target).next_back() {
            stats.checkpoint_hit = true;
            state.clone()
        } else {
            if target > self.limits.max_steps {
                return Err(SimulationError::BudgetExceeded);
            }
            let mut state = empty();
            let input = sample(config.start, &mut inputs, &mut stats)?;
            emit(config, &input, &mut state, &mut stats, self.limits)?;
            self.store(config, &state);
            state
        };
        if target.saturating_sub(state.tick) > self.limits.max_steps {
            return Err(SimulationError::BudgetExceeded);
        }
        while state.tick < target {
            let previous = tick_time(config, state.tick)?;
            let input = sample(previous, &mut inputs, &mut stats)?;
            let dt = config.step.as_time();
            let dt = dt.numerator() as f64 / dt.denominator() as f64;
            let updates = state.particles.len() as u64;
            if updates
                > self
                    .limits
                    .max_particle_updates
                    .saturating_sub(stats.particle_updates)
            {
                return Err(SimulationError::BudgetExceeded);
            }
            stats.particle_updates += updates;
            for particle in &mut state.particles {
                for axis in 0..2 {
                    particle.velocity[axis] += input.acceleration[axis] * dt;
                    particle.position[axis] += particle.velocity[axis] * dt;
                }
                if !particle
                    .position
                    .iter()
                    .chain(particle.velocity.iter())
                    .all(|v| v.is_finite())
                {
                    return Err(SimulationError::InvalidInput);
                }
            }
            state.tick += 1;
            stats.replayed_steps += 1;
            let time = tick_time(config, state.tick)?;
            let mut alive = Vec::with_capacity(state.particles.len());
            for particle in state.particles.drain(..) {
                if time.checked_sub(particle.birth)? < config.lifetime.as_time() {
                    alive.push(particle);
                }
            }
            state.particles = alive;
            if state.tick.is_multiple_of(stride) {
                let input = sample(time, &mut inputs, &mut stats)?;
                emit(config, &input, &mut state, &mut stats, self.limits)?;
            }
            self.store(config, &state);
        }
        Ok((state, stats))
    }
    fn store(&mut self, config: &SimulationConfig, state: &SimulationState) {
        if self.limits.max_checkpoints == 0
            || !state
                .tick
                .is_multiple_of(u64::from(config.checkpoint_stride))
            || state.particles.len() > self.limits.max_cached_particles
        {
            return;
        }
        self.checkpoints.insert(state.tick, state.clone());
        while self.checkpoints.len() > self.limits.max_checkpoints
            || self.cached_particle_count() > self.limits.max_cached_particles
        {
            self.checkpoints.pop_first();
        }
    }
}
fn empty() -> SimulationState {
    SimulationState {
        tick: 0,
        next_birth_serial: 0,
        particles: Vec::new(),
    }
}
fn validate<E>(c: &SimulationConfig) -> Result<u64, SimulationError<E>> {
    if c.step == Duration::ZERO
        || c.lifetime == Duration::ZERO
        || c.checkpoint_stride == 0
        || c.instance.ids().len() > 64
    {
        return Err(SimulationError::InvalidConfig);
    }
    let ratio = c
        .emission_interval
        .as_time()
        .checked_div(c.step.as_time())?;
    if ratio.denominator() != 1 || ratio.numerator() <= 0 {
        return Err(SimulationError::InvalidConfig);
    }
    Ok(ratio.numerator() as u64)
}
fn tick_time<E>(c: &SimulationConfig, tick: u64) -> Result<Time, SimulationError<E>> {
    let tick = i64::try_from(tick).map_err(|_| SimulationError::BudgetExceeded)?;
    Ok(c.start
        .checked_add(c.step.as_time().checked_mul(Time::from_integer(tick))?)?)
}
fn sample<E>(
    time: Time,
    f: &mut impl FnMut(Time) -> Result<ParticleInputs, E>,
    stats: &mut SimulationStats,
) -> Result<ParticleInputs, SimulationError<E>> {
    let input = f(time).map_err(SimulationError::Input)?;
    stats.sampled_inputs += 1;
    if !input
        .origin
        .iter()
        .chain(input.velocity.iter())
        .chain(input.jitter.iter())
        .chain(input.acceleration.iter())
        .all(|v| v.is_finite())
        || input.jitter.iter().any(|v| *v < 0.0)
    {
        return Err(SimulationError::InvalidInput);
    }
    Ok(input)
}
fn emit<E>(
    c: &SimulationConfig,
    input: &ParticleInputs,
    state: &mut SimulationState,
    stats: &mut SimulationStats,
    limits: SimulationLimits,
) -> Result<(), SimulationError<E>> {
    if !input.enabled {
        return Ok(());
    }
    let count = input.birth_count as usize;
    if count > limits.max_particles.saturating_sub(state.particles.len())
        || u64::from(input.birth_count) > limits.max_births.saturating_sub(stats.births)
    {
        return Err(SimulationError::BudgetExceeded);
    }
    for _ in 0..count {
        let serial = state.next_birth_serial;
        state.next_birth_serial = serial
            .checked_add(1)
            .ok_or(SimulationError::BudgetExceeded)?;
        let hash = birth_hash(c, serial);
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&hash[..16]);
        bytes[6] = (bytes[6] & 0x0f) | 0x80;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let mut velocity = input.velocity;
        for (axis, component) in velocity.iter_mut().enumerate() {
            let offset = 16 + axis * 8;
            let bits = u64::from_le_bytes(hash[offset..offset + 8].try_into().unwrap()) >> 11;
            let unit = bits as f64 / ((1u64 << 53) as f64);
            *component += (unit * 2.0 - 1.0) * input.jitter[axis];
        }
        if !velocity.iter().all(|v| v.is_finite()) {
            return Err(SimulationError::InvalidInput);
        }
        state.particles.push(Particle {
            id: CompositionInstanceId::from_uuid(Uuid::from_bytes(bytes)),
            birth: tick_time(c, state.tick)?,
            position: input.origin,
            velocity,
        });
        stats.births += 1;
    }
    Ok(())
}
fn base_hash(c: &SimulationConfig) -> Sha256 {
    let mut h = Sha256::new();
    h.update(b"kronello.simulation.v1");
    h.update(c.emitter.as_uuid().as_bytes());
    h.update((c.instance.ids().len() as u64).to_le_bytes());
    for id in c.instance.ids() {
        h.update(id.as_uuid().as_bytes());
    }
    h.update(c.seed.to_le_bytes());
    h
}
fn birth_hash(c: &SimulationConfig, serial: u64) -> [u8; 32] {
    let mut h = base_hash(c);
    h.update(serial.to_le_bytes());
    h.finalize().into()
}
fn identity(c: &SimulationConfig, inputs: [u8; 32]) -> [u8; 32] {
    let mut h = base_hash(c);
    h.update(inputs);
    for t in [
        c.start,
        c.step.as_time(),
        c.lifetime.as_time(),
        c.emission_interval.as_time(),
    ] {
        h.update(t.numerator().to_le_bytes());
        h.update(t.denominator().to_le_bytes());
    }
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> SimulationConfig {
        SimulationConfig {
            emitter: ContentId::from_uuid(Uuid::from_u128(1)),
            instance: InstancePath::new(vec![CompositionInstanceId::from_uuid(Uuid::from_u128(2))]),
            seed: 7,
            start: Time::new(1, 3).unwrap(),
            step: Duration::new(Time::new(1, 10).unwrap()).unwrap(),
            lifetime: Duration::new(Time::new(1, 2).unwrap()).unwrap(),
            emission_interval: Duration::new(Time::new(1, 5).unwrap()).unwrap(),
            checkpoint_stride: 2,
        }
    }
    fn input(t: Time) -> Result<ParticleInputs, &'static str> {
        Ok(ParticleInputs {
            origin: [t.numerator() as f64 / t.denominator() as f64, 0.0],
            velocity: [1.0, 0.0],
            jitter: [0.2, 0.1],
            acceleration: [0.0, -2.0],
            enabled: true,
            birth_count: 1,
        })
    }
    #[test]
    fn cold_cached_reverse_random_and_disabled_cache_are_identical() {
        let c = config();
        let hash = [3; 32];
        let mut cache = SimulationCache::new(SimulationLimits::default());
        for tick in [0, 1, 7, 4, 3, 9, 1, 0, 9, 8] {
            let time = tick_time::<()>(&c, tick).unwrap();
            let actual = cache.state_at(&c, hash, time, input).unwrap().0;
            let mut cold = SimulationCache::new(SimulationLimits {
                max_checkpoints: 0,
                ..SimulationLimits::default()
            });
            let expected = cold.state_at(&c, hash, time, input).unwrap().0;
            assert_eq!(actual, expected);
            assert!(cache.checkpoint_count() <= 32);
        }
        let (_, stats) = cache
            .state_at(&c, hash, tick_time::<()>(&c, 8).unwrap(), input)
            .unwrap();
        assert!(stats.checkpoint_hit);
        assert_eq!(stats.replayed_steps, 0);
    }
    #[test]
    fn exact_grid_lifetime_euler_and_stable_birth_identity() {
        let mut c = config();
        c.start = Time::ZERO;
        let fixed = |_| {
            Ok::<_, ()>(ParticleInputs {
                origin: [0.0, 0.0],
                velocity: [1.0, 0.0],
                jitter: [0.0, 0.0],
                acceleration: [0.0, 2.0],
                enabled: true,
                birth_count: 1,
            })
        };
        let mut cache = SimulationCache::new(SimulationLimits::default());
        let zero = cache.state_at(&c, [1; 32], Time::ZERO, fixed).unwrap().0;
        assert_eq!(zero.particles.len(), 1);
        let a = cache
            .state_at(&c, [1; 32], Time::new(1, 10).unwrap(), fixed)
            .unwrap()
            .0;
        assert_eq!(a.particles[0].id, zero.particles[0].id);
        assert_eq!(a.particles[0].position, [0.1, 0.020000000000000004]);
        let fractional = cache
            .state_at(&c, [1; 32], Time::new(19, 100).unwrap(), fixed)
            .unwrap()
            .0;
        assert_eq!(a, fractional);
        let end = cache
            .state_at(&c, [1; 32], Time::new(1, 2).unwrap(), fixed)
            .unwrap()
            .0;
        assert!(!end.particles.iter().any(|p| p.id == zero.particles[0].id));
        assert_eq!(end.next_birth_serial, 3);
        let before = cache
            .state_at(&c, [1; 32], Time::new(-1, 10).unwrap(), fixed)
            .unwrap()
            .0;
        assert!(before.particles.is_empty());
        let mut other = c.clone();
        other.seed += 1;
        let changed = cache
            .state_at(&other, [1; 32], Time::ZERO, fixed)
            .unwrap()
            .0;
        assert_ne!(changed.particles[0].id, zero.particles[0].id);
        other = c.clone();
        other.instance = other
            .instance
            .child(CompositionInstanceId::from_uuid(Uuid::from_u128(9)));
        assert_ne!(
            cache
                .state_at(&other, [1; 32], Time::ZERO, fixed)
                .unwrap()
                .0
                .particles[0]
                .id,
            zero.particles[0].id
        );
    }
    #[test]
    fn input_hash_invalidation_bounds_and_typed_errors() {
        let c = config();
        let mut cache = SimulationCache::new(SimulationLimits {
            max_checkpoints: 2,
            max_cached_particles: 4,
            ..SimulationLimits::default()
        });
        let t = tick_time::<()>(&c, 8).unwrap();
        let old = cache.state_at(&c, [1; 32], t, input).unwrap().0;
        let changed_input = |time| {
            let mut v = input(time)?;
            v.acceleration = [5.0, 4.0];
            Ok::<_, &'static str>(v)
        };
        let changed = cache.state_at(&c, [2; 32], t, changed_input).unwrap().0;
        let cold = SimulationCache::new(SimulationLimits::default())
            .state_at(&c, [2; 32], t, changed_input)
            .unwrap()
            .0;
        assert_eq!(changed, cold);
        assert_ne!(changed, old);
        assert!(cache.checkpoint_count() <= 2);
        assert!(cache.cached_particle_count() <= 4);
        let mut budget = SimulationCache::new(SimulationLimits {
            max_steps: 2,
            ..SimulationLimits::default()
        });
        assert!(matches!(
            budget.state_at(&c, [1; 32], t, input),
            Err(SimulationError::BudgetExceeded)
        ));
        let mut births = SimulationCache::new(SimulationLimits {
            max_births: 0,
            ..SimulationLimits::default()
        });
        assert!(matches!(
            births.state_at(&c, [1; 32], c.start, input),
            Err(SimulationError::BudgetExceeded)
        ));
        let mut bad = c.clone();
        bad.step = Duration::ZERO;
        assert!(matches!(
            cache.state_at(&bad, [1; 32], t, input),
            Err(SimulationError::InvalidConfig)
        ));
        assert!(matches!(
            cache.state_at(&c, [4; 32], t, |_| Err("missing")),
            Err(SimulationError::Input("missing"))
        ));
        assert!(matches!(
            cache.state_at(&c, [5; 32], t, |time| {
                let mut v = input(time)?;
                v.origin[0] = f64::NAN;
                Ok::<_, &'static str>(v)
            }),
            Err(SimulationError::InvalidInput)
        ));
    }
    #[test]
    fn ntsc_negative_start_birth_edges_and_disabled_emission_preserve_serials() {
        let mut c = config();
        c.start = Time::new(-1, 3).unwrap();
        c.step = Duration::new(Time::new(1001, 30000).unwrap()).unwrap();
        c.emission_interval =
            Duration::new(c.step.as_time().checked_mul(Time::from_integer(2)).unwrap()).unwrap();
        c.lifetime = c.emission_interval;
        let mut seen = Vec::new();
        let mut cache = SimulationCache::new(SimulationLimits::default());
        let at_two = tick_time::<()>(&c, 2).unwrap();
        let state = cache
            .state_at(&c, [7; 32], at_two, |time| {
                seen.push(time);
                let mut value = input(time)?;
                value.enabled = time != c.start;
                value.birth_count = 2;
                Ok::<_, &'static str>(value)
            })
            .unwrap()
            .0;
        assert_eq!(state.next_birth_serial, 2);
        assert_eq!(state.particles.len(), 2);
        assert!(state.particles.iter().all(|p| p.birth == at_two));
        assert_ne!(state.particles[0].id, state.particles[1].id);
        assert!(seen.iter().all(|t| {
            t.checked_sub(c.start)
                .unwrap()
                .checked_div(c.step.as_time())
                .unwrap()
                .denominator()
                == 1
        }));
        let before = at_two
            .checked_sub(Time::new(1, 1_000_000_000).unwrap())
            .unwrap();
        let previous = cache
            .state_at(&c, [7; 32], before, |time| {
                let mut value = input(time)?;
                value.enabled = time != c.start;
                value.birth_count = 2;
                Ok::<_, &'static str>(value)
            })
            .unwrap()
            .0;
        assert_eq!(previous.tick, 1);
        assert!(previous.particles.is_empty());
        let at_four = cache
            .state_at(&c, [7; 32], tick_time::<()>(&c, 4).unwrap(), |time| {
                let mut value = input(time)?;
                value.enabled = time != c.start;
                value.birth_count = 2;
                Ok::<_, &'static str>(value)
            })
            .unwrap()
            .0;
        assert!(
            at_four
                .particles
                .iter()
                .all(|p| p.birth == tick_time::<()>(&c, 4).unwrap())
        );
        assert!(
            at_four
                .particles
                .iter()
                .all(|p| !state.particles.iter().any(|old| old.id == p.id))
        );
    }
    #[test]
    fn failed_requests_do_not_poison_checkpoints_and_update_work_is_bounded() {
        let c = config();
        let hash = [5; 32];
        let t = tick_time::<()>(&c, 8).unwrap();
        let mut cache = SimulationCache::new(SimulationLimits::default());
        let failure_tick = tick_time::<()>(&c, 5).unwrap();
        assert!(matches!(
            cache.state_at(&c, hash, t, |time| {
                if time == failure_tick {
                    Err("failed tick")
                } else {
                    input(time)
                }
            }),
            Err(SimulationError::Input("failed tick"))
        ));
        let recovered = cache.state_at(&c, hash, t, input).unwrap().0;
        let cold = SimulationCache::new(SimulationLimits::default())
            .state_at(&c, hash, t, input)
            .unwrap()
            .0;
        assert_eq!(recovered, cold);
        let mut capped = SimulationCache::new(SimulationLimits {
            max_particle_updates: 1,
            ..SimulationLimits::default()
        });
        assert!(matches!(
            capped.state_at(&c, hash, t, input),
            Err(SimulationError::BudgetExceeded)
        ));
        let one = capped
            .state_at(&c, hash, tick_time::<()>(&c, 1).unwrap(), input)
            .unwrap();
        assert_eq!(one.1.particle_updates, 1);
        let mut no_steps = SimulationCache::new(SimulationLimits {
            max_steps: 0,
            ..SimulationLimits::default()
        });
        assert!(matches!(
            no_steps.state_at(&c, hash, t, |_| -> Result<ParticleInputs, ()> {
                panic!("budget must reject before input callback")
            }),
            Err(SimulationError::BudgetExceeded)
        ));
        assert_eq!(no_steps.checkpoint_count(), 0);
    }
    #[test]
    fn configuration_identity_changes_replay_and_cache_stride_is_nonsemantic() {
        let c = config();
        let hash = [8; 32];
        let t = tick_time::<()>(&c, 8).unwrap();
        let mut cache = SimulationCache::new(SimulationLimits::default());
        cache.state_at(&c, hash, t, input).unwrap();
        let mut variants = [c.clone(), c.clone(), c.clone(), c.clone()];
        variants[0].lifetime = Duration::new(Time::new(2, 5).unwrap()).unwrap();
        variants[1].start = Time::ZERO;
        variants[2].emission_interval = c.step;
        variants[3].step = Duration::new(Time::new(1, 20).unwrap()).unwrap();
        for changed in variants {
            let (warm, stats) = cache.state_at(&changed, hash, t, input).unwrap();
            let cold = SimulationCache::new(SimulationLimits::default())
                .state_at(&changed, hash, t, input)
                .unwrap()
                .0;
            assert_eq!(warm, cold);
            assert!(!stats.checkpoint_hit);
        }
        let original = SimulationCache::new(SimulationLimits::default())
            .state_at(&c, hash, t, input)
            .unwrap()
            .0;
        let mut different_stride = c.clone();
        different_stride.checkpoint_stride = 1;
        let other = SimulationCache::new(SimulationLimits::default())
            .state_at(&different_stride, hash, t, input)
            .unwrap()
            .0;
        assert_eq!(original, other);
        let mut invalid = c.clone();
        invalid.emission_interval = Duration::new(Time::new(3, 20).unwrap()).unwrap();
        assert!(matches!(
            cache.state_at(&invalid, hash, t, input),
            Err(SimulationError::InvalidConfig)
        ));
    }
    #[test]
    fn particle_capacity_exact_boundary_and_expiration_releases_capacity() {
        let mut c = config();
        c.lifetime = Duration::new(Time::from_integer(1)).unwrap();
        let limits = SimulationLimits {
            max_particles: 2,
            ..SimulationLimits::default()
        };
        let mut cache = SimulationCache::new(limits);
        let full = cache
            .state_at(&c, [9; 32], tick_time::<()>(&c, 2).unwrap(), input)
            .unwrap()
            .0;
        assert_eq!(full.particles.len(), 2);
        assert!(matches!(
            cache.state_at(&c, [9; 32], tick_time::<()>(&c, 4).unwrap(), input),
            Err(SimulationError::BudgetExceeded)
        ));
        c.lifetime = c.emission_interval;
        let mut one = SimulationCache::new(SimulationLimits {
            max_particles: 1,
            ..SimulationLimits::default()
        });
        let expired = one
            .state_at(&c, [9; 32], tick_time::<()>(&c, 4).unwrap(), input)
            .unwrap()
            .0;
        assert_eq!(expired.particles.len(), 1);
        assert_eq!(expired.next_birth_serial, 3);
        assert_eq!(expired.particles[0].birth, tick_time::<()>(&c, 4).unwrap());
    }
}
