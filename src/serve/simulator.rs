//! A reproducible concrete run of the transition engine. Only fast-forward
//! transitions enter the trace; the remaining ranks prepare the next move.

use num_bigint::BigUint;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive};

use crate::num::{zeros, Exact};
use crate::state::{Engine, State, DONE};
use crate::tree::{Kind, Tree};

use super::{assemble, editing_name, err, esc, field, name_of, node_json, Bench};

const MAX_NODES: usize = 4096;
const MAX_STEPS: usize = 4096;
const MAX_CELLS: usize = 2_000_000;
const MAX_EXACT_BITS: u64 = 16 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;

struct Options {
    seed: u64,
    steps: usize,
    actions: Vec<u32>,
    time: bool,
}

impl Options {
    fn read(body: &str) -> Result<Self, String> {
        fn integer(body: &str, key: &str, default: &str) -> Result<u64, String> {
            let text = field(body, key).unwrap_or_else(|| default.to_string());
            let text = text.trim();
            if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit()) {
                return Err(format!("{key} must be an unsigned whole number"));
            }
            text.parse()
                .map_err(|_| format!("{key} is outside the unsigned 64-bit range"))
        }
        let seed = integer(body, "seed", "1")?;
        let steps = integer(body, "steps", "0")?;
        if steps > MAX_STEPS as u64 {
            return Err(format!(
                "a simulation accepts at most {MAX_STEPS} fast-forward steps"
            ));
        }
        let text = field(body, "actions").unwrap_or_default();
        let mut actions = Vec::new();
        if !text.trim().is_empty() {
            for part in text.split(',') {
                let part = part.trim();
                if part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()) {
                    return Err("actions must be comma-separated child identifiers".to_string());
                }
                let id: u32 = part
                    .parse()
                    .map_err(|_| "an action identifier is too large")?;
                if id == 0 {
                    return Err("an action identifier must be positive".to_string());
                }
                actions.push(id);
                if actions.len() > MAX_NODES {
                    return Err("there are too many simulation actions".to_string());
                }
            }
        }
        let time = match field(body, "time").as_deref() {
            None | Some("0") | Some("false") => false,
            Some("1") | Some("true") => true,
            _ => return Err("time must be 0 or 1".to_string()),
        };
        Ok(Self {
            seed,
            steps: steps as usize,
            actions,
            time,
        })
    }
}

/// SplitMix64 supplies the bits, then rejection sampling chooses an integer
/// below the exact denominator. No rounding of a probability affects a draw.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: &BigUint) -> BigUint {
        let bits = (bound - BigUint::one()).bits() as usize;
        let bytes = (bits + 7) / 8;
        loop {
            let mut candidate = Vec::with_capacity(bytes);
            while candidate.len() < bytes {
                let word = self.next().to_le_bytes();
                let take = (bytes - candidate.len()).min(word.len());
                candidate.extend_from_slice(&word[..take]);
            }
            if bits % 8 != 0 {
                candidate[bytes - 1] &= (1u8 << (bits % 8)) - 1;
            }
            let value = BigUint::from_bytes_le(&candidate);
            if &value < bound {
                return value;
            }
        }
    }

    fn low(&mut self, probability: &BigRational) -> bool {
        self.below(probability.denom().magnitude()) < *probability.numer().magnitude()
    }
}

struct Snapshot {
    time: u32,
    delta: u32,
    probability: BigRational,
    impact: Vec<Exact>,
    state: State,
    running: Vec<u32>,
    completed: Vec<u32>,
}

#[derive(Debug, PartialEq, Eq)]
enum Status {
    Ready,
    Choice(u32),
    Complete,
}

struct Trace {
    steps: Vec<Snapshot>,
    probability: BigRational,
    impact: Vec<Exact>,
    elapsed: u32,
    status: Status,
    actions: Vec<u32>,
}

fn rational_bits(value: &BigRational) -> u64 {
    value.numer().bits().saturating_add(value.denom().bits())
}

fn replay(tree: &Tree, options: &Options) -> Result<Trace, String> {
    let possible_steps = options.steps.min(tree.n_tasks as usize);
    if (tree.n_nodes as usize + 1 + tree.k).saturating_mul(possible_steps) > MAX_CELLS {
        return Err(
            "the simulation trace is too large; request fewer fast-forward steps".to_string(),
        );
    }
    let engine = Engine::new(tree);
    let mut state = engine.initial_state();
    let mut random = Random(options.seed);
    let mut probability = BigRational::one();
    let mut impact = zeros::<Exact>(tree.k);
    let mut steps = Vec::new();
    let mut elapsed = 0;
    let mut used_actions = 0;
    let mut exact_bits = 0;
    let status = loop {
        match engine.rank_witness(&state) {
            None if engine.is_final(&state) => break Status::Complete,
            None => return Err("the simulation reached a state with no transition".to_string()),
            Some((1, id)) => {
                let Some(&action) = options.actions.get(used_actions) else {
                    break Status::Choice(id);
                };
                let node = tree.node(id);
                if action != node.low && action != node.high {
                    return Err(format!(
                        "action {action} cannot resolve choice {id}; take child {} or {}",
                        node.low, node.high
                    ));
                }
                state[action as usize] = 0;
                used_actions += 1;
            }
            Some((2, id)) => engine.down(&mut state, id),
            Some((3, id)) => engine.up(&mut state, id),
            Some((4, id)) => {
                let node = tree.node(id);
                let p = tree.prob_exact(id);
                if random.low(p) {
                    state[node.low as usize] = 0;
                    probability *= p;
                } else {
                    state[node.high as usize] = 0;
                    probability *= BigRational::one() - p;
                }
                if rational_bits(&probability) > MAX_EXACT_BITS {
                    return Err("the exact simulation probability is too large".to_string());
                }
            }
            Some((5, _)) => {
                // Check the requested length only at the next fast-forward:
                // hidden closures still run after the last visible snapshot.
                if steps.len() == options.steps {
                    break Status::Ready;
                }
                let running: Vec<u32> = tree
                    .tasks
                    .iter()
                    .copied()
                    .filter(|&id| state[id as usize] >= 0)
                    .collect();
                let delta = running
                    .iter()
                    .map(|&id| (tree.node(id).duration as i32 - state[id as usize] as i32) as u32)
                    .min()
                    .ok_or("a fast-forward has no running task")?;
                engine.fast_forward::<Exact>(&mut state, &mut impact);
                elapsed += delta;
                let completed = running
                    .iter()
                    .copied()
                    .filter(|&id| state[id as usize] == DONE)
                    .collect();
                exact_bits += rational_bits(&probability)
                    + impact.iter().map(|v| rational_bits(&v.0)).sum::<u64>();
                if exact_bits > MAX_EXACT_BITS {
                    return Err(
                        "the exact simulation trace is too large; request fewer steps".to_string(),
                    );
                }
                steps.push(Snapshot {
                    time: elapsed,
                    delta,
                    probability: probability.clone(),
                    impact: impact.clone(),
                    state: state.clone(),
                    running,
                    completed,
                });
            }
            Some((rank, _)) => return Err(format!("unknown simulation transition rank {rank}")),
        }
    };
    if used_actions != options.actions.len() {
        return Err("actions remain beyond the requested simulation trace".to_string());
    }
    Ok(Trace {
        steps,
        probability,
        impact,
        elapsed,
        status,
        actions: options.actions.clone(),
    })
}

fn integers<T: std::fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn impacts(items: &[Exact]) -> String {
    items
        .iter()
        .map(|v| esc(&v.0.to_string()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn label(tree: &Tree, labels: &[String], id: u32) -> String {
    labels
        .get(id as usize)
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            if tree.node(id).kind == Kind::Task {
                name_of(tree, id)
            } else {
                id.to_string()
            }
        })
}

fn response(tree: &Tree, labels: &[String], seed: u64, trace: &Trace) -> Result<String, String> {
    let mut steps = String::new();
    for (i, step) in trace.steps.iter().enumerate() {
        if i > 0 {
            steps.push(',');
        }
        steps.push_str(&format!(
            "{{\"index\": {}, \"time\": {}, \"delta\": {}, \"probability\": {}, \
             \"probability_approx\": {}, \"impact\": [{}], \"state\": [{}], \
             \"running\": [{}], \"completed\": [{}]}}",
            i + 1,
            step.time,
            step.delta,
            esc(&step.probability.to_string()),
            step.probability.to_f64().unwrap_or(0.0),
            impacts(&step.impact),
            integers(&step.state),
            integers(&step.running),
            integers(&step.completed)
        ));
        if steps.len() > MAX_JSON_BYTES {
            return Err("the simulation response is too large; request fewer steps".to_string());
        }
    }
    let names = (0..tree.k)
        .map(|c| match tree.meta.impact_names.get(c) {
            Some(name) if !name.is_empty() => esc(name),
            _ => esc(&format!("impact {c}")),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let nodes = (1..=tree.n_nodes)
        .map(|id| {
            format!(
                "{{\"id\": {id}, \"label\": {}, \"name\": {}, \"task\": {}, \"duration\": {}}}",
                esc(&label(tree, labels, id)),
                esc(&name_of(tree, id)),
                tree.node(id).kind == Kind::Task,
                tree.node(id).duration
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let (status, choice) = match trace.status {
        Status::Ready => ("ready", "null".to_string()),
        Status::Complete => ("complete", "null".to_string()),
        Status::Choice(id) => {
            let node = tree.node(id);
            (
                "choice",
                format!(
                    "{{\"id\": {id}, \"label\": {}, \"low\": {}, \"high\": {}}}",
                    esc(&label(tree, labels, id)),
                    node_json(tree, node.low, labels),
                    node_json(tree, node.high, labels)
                ),
            )
        }
    };
    let result = format!(
        "{{\"seed\": {}, \"steps\": [{}], \"names\": [{}], \"nodes\": [{}], \
         \"probability\": {}, \"probability_approx\": {}, \"impact\": [{}], \
         \"elapsed\": {}, \"status\": {}, \"choice\": {}, \"actions\": [{}]}}",
        esc(&seed.to_string()),
        steps,
        names,
        nodes,
        esc(&trace.probability.to_string()),
        trace.probability.to_f64().unwrap_or(0.0),
        impacts(&trace.impact),
        trace.elapsed,
        esc(status),
        choice,
        integers(&trace.actions)
    );
    if result.len() > MAX_JSON_BYTES {
        return Err("the simulation response is too large; request fewer steps".to_string());
    }
    Ok(result)
}

fn run(body: &str, bench: &Bench) -> Result<String, String> {
    let options = Options::read(body)?;
    let text = field(body, "process")
        .filter(|s| !s.trim().is_empty())
        .ok_or("no process was given")?;
    if text.len() > super::MAX_BODY {
        return Err("the process is too large to simulate".to_string());
    }
    let editing = editing_name(body);
    let parsed = assemble(&text, bench, editing.as_deref())?;
    if parsed.labels.len().saturating_sub(1) > MAX_NODES {
        return Err(format!(
            "a simulation accepts at most {MAX_NODES} process nodes"
        ));
    }
    // Bound the dense impact table before the tree allocates it. This header
    // comes from our expression parser, never directly from the request.
    let dimensions: usize = parsed
        .yaml
        .lines()
        .find_map(|line| {
            line.strip_prefix("dimensions: ")
                .and_then(|n| n.parse().ok())
        })
        .ok_or("the process has no impact dimension")?;
    if parsed
        .labels
        .len()
        .saturating_mul(dimensions.saturating_add(usize::from(options.time)))
        > MAX_CELLS
    {
        return Err("the process has too many impact components to simulate".to_string());
    }
    let tree = Tree::parse(&parsed.yaml).map_err(|e| format!("the instance does not read: {e}"))?;
    let tree = if options.time { tree.with_time() } else { tree };
    let trace = replay(&tree, &options)?;
    response(&tree, &parsed.labels, options.seed, &trace)
}

pub(super) fn simulate(body: &str, bench: &Bench) -> String {
    match run(body, bench) {
        Ok(result) => result,
        Err(message) => err(&message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num::parse_decimal;

    fn tree(text: &str) -> Tree {
        Tree::parse(&crate::parse::to_yaml(text).unwrap().yaml).unwrap()
    }

    fn options(steps: usize, actions: &[u32]) -> Options {
        Options {
            seed: 1,
            steps,
            actions: actions.to_vec(),
            time: false,
        }
    }

    #[test]
    fn parallel_fast_forwards_preserve_progress_and_actual_impact() {
        let tree = tree("((short, 2, {cost: 1.5}) || (long, 5, {cost: 2.25}))").with_time();
        let trace = replay(&tree, &options(2, &[])).unwrap();
        assert_eq!(trace.status, Status::Complete);
        assert_eq!(trace.elapsed, 5);
        assert_eq!(trace.steps.len(), 2);
        assert_eq!(trace.steps[0].delta, 2);
        assert_eq!(trace.steps[0].state, vec![-1, DONE, 0, 2]);
        assert_eq!(trace.steps[0].running, vec![1, 3]);
        assert_eq!(trace.steps[0].completed, vec![1]);
        assert_eq!(impacts(&trace.steps[0].impact), "\"2\", \"3/2\"");
        assert_eq!(trace.steps[1].delta, 3);
        assert_eq!(trace.steps[1].completed, vec![3]);
        assert_eq!(impacts(&trace.impact), "\"5\", \"15/4\"");
        assert_eq!(trace.probability, BigRational::one());
    }

    #[test]
    fn simultaneous_completions_are_one_visible_step() {
        let tree = tree("((a, 3, {x: 2}) || (b, 3, {x: 4}))");
        let trace = replay(&tree, &options(1, &[])).unwrap();
        assert_eq!(trace.status, Status::Complete);
        assert_eq!(trace.steps.len(), 1);
        assert_eq!(trace.steps[0].completed, vec![1, 3]);
        assert_eq!(impacts(&trace.impact), "\"6\"");
    }

    #[test]
    fn choices_pause_without_a_visible_housekeeping_state() {
        let tree = tree("((a, 1, {x: 2}), ((b, 1, {x: 3}) ^ (c, 2, {x: 5})))");
        let trace = replay(&tree, &options(1, &[])).unwrap();
        assert_eq!(trace.status, Status::Choice(4));
        assert_eq!(trace.steps.len(), 1);
        assert_eq!(trace.steps[0].completed, vec![1]);
        for (action, expected) in [(3, "\"5\""), (5, "\"7\"")] {
            let trace = replay(&tree, &options(2, &[action])).unwrap();
            assert_eq!(trace.status, Status::Complete);
            assert_eq!(trace.probability, BigRational::one());
            assert_eq!(impacts(&trace.impact), expected);
            assert_eq!(trace.actions, vec![action]);
        }
        assert!(replay(&tree, &options(2, &[1])).is_err());
        assert!(replay(&tree, &options(0, &[3])).is_err());
        assert!(replay(&tree, &options(2, &[3, 5])).is_err());
    }

    #[test]
    fn nature_is_sampled_exactly_and_replays_prefixes() {
        let tree = tree(
            "(((a, 1, {x: 1}) ^[0.2] (b, 1, {x: 2})), ((c, 1, {x: 4}) ^[0.3] (d, 1, {x: 8})))",
        );
        let mut saw_low = false;
        let mut saw_high = false;
        for seed in 0..64 {
            let mut opts = options(2, &[]);
            opts.seed = seed;
            let trace = replay(&tree, &opts).unwrap();
            assert_eq!(trace.status, Status::Complete);
            let first_low = trace.steps[0].completed == [1];
            let second_low = trace.steps[1].completed == [5];
            saw_low |= first_low;
            saw_high |= !first_low;
            let first = parse_decimal(if first_low { "0.2" } else { "0.8" }).unwrap();
            let second = parse_decimal(if second_low { "0.3" } else { "0.7" }).unwrap();
            assert_eq!(trace.steps[0].probability, first);
            assert_eq!(trace.probability, &first * &second);
            assert_eq!(trace.steps[1].probability, trace.probability);
            opts.steps = 1;
            let prefix = replay(&tree, &opts).unwrap();
            assert_eq!(prefix.status, Status::Ready);
            assert_eq!(prefix.steps[0].state, trace.steps[0].state);
            assert_eq!(prefix.steps[0].probability, first);
            // The next nature branch has already been drawn in preparation
            // for the next FF, without rewriting the historical snapshot.
            assert_eq!(prefix.probability, trace.probability);
        }
        assert!(saw_low && saw_high);
    }

    #[test]
    fn zero_steps_prepares_the_first_boundary() {
        let task = tree("(a, 2, {x: 1})");
        let trace = replay(&task, &options(0, &[])).unwrap();
        assert_eq!(trace.status, Status::Ready);
        assert!(trace.steps.is_empty());
        assert_eq!(trace.elapsed, 0);
        assert_eq!(impacts(&trace.impact), "\"0\"");
        let choice = tree("((a, 2, {x: 1}) ^ (b, 1, {x: 2}))");
        let trace = replay(&choice, &options(0, &[])).unwrap();
        assert_eq!(trace.status, Status::Choice(2));
        assert!(trace.steps.is_empty());
    }

    #[test]
    fn request_arguments_are_strict_and_seed_keeps_all_64_bits() {
        let opts = Options::read("").unwrap();
        assert_eq!(opts.seed, 1);
        assert_eq!(opts.steps, 0);
        for body in [
            "seed=-1",
            "seed=",
            "seed=18446744073709551616",
            "steps=1.5",
            "steps=4097",
            "actions=0",
            "actions=1,,3",
            "time=maybe",
        ] {
            assert!(Options::read(body).is_err(), "accepted {body}");
        }
        let opts = Options::read("seed=18446744073709551615").unwrap();
        assert_eq!(opts.seed, u64::MAX);
        let tree = tree("(a, 1, {x: 1})");
        let trace = replay(&tree, &opts).unwrap();
        let out = response(&tree, &[], opts.seed, &trace).unwrap();
        assert!(out.contains("\"seed\": \"18446744073709551615\""));
    }
}
