//! Cubic position kernel and the main-path distance lookup.

type Point = [f32; 3];
const ARC_STEPS: usize = 200;

#[derive(Clone, Copy)]
struct Hermite {
    start: Point,
    outgoing: Point,
    incoming: Point,
    end: Point,
}

impl Hermite {
    fn sample(self, t: f32) -> Point {
        // REL .text 3c0fbc..3c1088. Maintain the original multiply, fused
        // multiply-add, and add order independently for each coordinate.
        let squared = t * t;
        let cubed = t * squared;
        let twice_cubed = 2. * cubed;
        let twice_squared = 2. * squared;
        let thrice_squared = 3. * squared;
        std::array::from_fn(|axis| {
            let start = self.start[axis];
            let outgoing = self.outgoing[axis];
            let incoming = self.incoming[axis];
            let end = self.end[axis];
            let value = (-start).mul_add(thrice_squared, start.mul_add(twice_cubed, 0.)) + start;
            let value = outgoing.mul_add(cubed, value);
            let value = (-outgoing).mul_add(twice_squared, value);
            let value = outgoing.mul_add(t, value);
            let value = incoming.mul_add(cubed, value);
            let value = (-incoming).mul_add(squared, value);
            let value = (-end).mul_add(twice_cubed, value);
            end.mul_add(thrice_squared, value)
        })
    }
}

pub(super) fn ease_phase(phase: f32, no_start: bool, no_end: bool) -> f32 {
    // REL .text 3c12b4..3c1434 integrates a speed ramp over each enabled
    // endpoint window. Preserve the single-precision operation order.
    const WINDOW: f32 = 0.1;
    const END_SPEED: f32 = 0.01;
    if no_start && no_end {
        return phase;
    }
    let window_sum = if !no_start && !no_end { WINDOW } else { 0.5 * WINDOW };
    let speed = (-window_sum).mul_add(END_SPEED, 1.) / (1. - window_sum);
    let eased_average = |ratio: f32| 0.5 * (END_SPEED + ratio.mul_add(speed - END_SPEED, END_SPEED));
    if !no_start && phase < WINDOW {
        phase * eased_average(phase / WINDOW)
    } else if !no_end && phase > 1. - WINDOW {
        let remaining = 1. - phase;
        (-remaining).mul_add(eased_average(remaining / WINDOW), 1.)
    } else if no_start {
        phase * speed
    } else {
        speed.mul_add(phase - WINDOW, WINDOW * (0.5 * (END_SPEED + speed)))
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct PhaseStep {
    pub(super) phase: f32,
    pub(super) boundary: i8,
    pub(super) residual: f32,
}

pub(super) fn phase_step(phase: f32, dt: f32, length: f32, duration: f32, reversed: bool) -> PhaseStep {
    // REL 3bc84c..3bc9fc reports a crossing only after leaving the phase range.
    let direction = if reversed { -1. } else { 1. };
    if length < 0.001 {
        let delta = (1. - phase) * direction;
        return PhaseStep { phase: (phase + delta).clamp(0., 1.), boundary: 0, residual: 0. };
    }
    let delta = (dt * duration.recip()) * direction;
    let next = phase + delta;
    let (boundary, excess) = if next > 1. {
        (1, next - 1.)
    } else if next < 0. {
        (-1, next)
    } else {
        return PhaseStep { phase: next, boundary: 0, residual: 0. };
    };
    PhaseStep {
        phase: (phase + (delta - excess)).clamp(0., 1.),
        boundary,
        residual: (excess * duration).abs(),
    }
}

pub(super) struct Path {
    curve: Hermite,
    main_path: bool,
    lengths: Option<Box<[f32; ARC_STEPS]>>,
    total: f32,
}

impl Path {
    pub(super) fn new(start: Point, outgoing: Point, incoming: Point, end: Point, main_path: bool) -> Self {
        let mut path = Self {
            curve: Hermite { start, outgoing, incoming, end },
            main_path,
            lengths: None,
            total: 0.,
        };
        path.rebuild_lengths();
        path
    }

    fn rebuild_lengths(&mut self) {
        // Both paths measure 200 chords (REL 3c177c and 3c18d0). Only
        // the main path keeps them for distance-to-parameter lookup.
        let straight = self.curve.incoming == [0.; 3] && self.curve.outgoing == [0.; 3];
        if straight {
            self.total = distance(self.curve.start, self.curve.end);
            return;
        }
        let mut lengths = self.main_path.then(|| self.lengths.get_or_insert_with(|| Box::new([0.; ARC_STEPS])));
        let mut previous = self.curve.start;
        self.total = 0.;
        for i in 0..ARC_STEPS {
            let next = self.curve.sample((i + 1) as f32 / ARC_STEPS as f32);
            let length = distance(previous, next);
            self.total += length;
            if let Some(entries) = &mut lengths {
                entries[i] = length;
            }
            previous = next;
        }
    }

    pub(super) fn update_endpoints(&mut self, start: Point, end: Point) {
        if self.curve.start != start || self.curve.end != end {
            self.curve.start = start;
            self.curve.end = end;
            self.rebuild_lengths();
        }
    }

    pub(super) fn length(&self) -> f32 { self.total }

    pub(super) fn duration(&self, authored: f32, speed: f32) -> f32 {
        // Knot preparation clamps explicit times at length / 200. Omitted
        // time divides the cached route length by the current speed.
        if authored > f32::EPSILON {
            authored.max(self.total / ARC_STEPS as f32)
        } else {
            self.total / speed
        }
    }

    pub(super) fn sample(&self, phase: f32) -> Point {
        if self.main_path && self.lengths.is_none() {
            let [start, end] = [self.curve.start, self.curve.end];
            return std::array::from_fn(|axis| start[axis] + (end[axis] - start[axis]) * phase);
        }
        self.curve.sample(self.parameter(phase))
    }

    fn parameter(&self, phase: f32) -> f32 {
        let Some(lengths) = &self.lengths else { return phase };
        let distance = phase * self.total;
        let mut passed = 0.;
        for (i, &length) in lengths.iter().enumerate() {
            let next = passed + length;
            if next > distance {
                return (i as f32 + (distance - passed) / length) / ARC_STEPS as f32;
            }
            passed = next;
        }
        phase
    }
}

fn distance(start: Point, end: Point) -> f32 {
    let [x, y, z] = std::array::from_fn(|axis| end[axis] - start[axis]);
    (z.mul_add(z, x * x) + y * y).sqrt()
}

#[cfg(debug_assertions)]
pub(super) fn validate() -> anyhow::Result<()> {
    #[derive(serde::Deserialize)]
    struct Sample {
        t: f32,
        controls: [Point; 4],
        position: [String; 3],
    }
    let samples: Vec<Sample> = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/spline_curve_kernel.json"
    ))?;
    for sample in samples {
        let [start, outgoing, incoming, end] = sample.controls;
        let point = Hermite { start, outgoing, incoming, end }.sample(sample.t);
        for (actual, expected) in point.into_iter().zip(sample.position) {
            anyhow::ensure!(actual.to_bits() == u32::from_str_radix(&expected, 16)?,
                "Spline kernel differs at phase {}: {:08x} vs {expected}", sample.t, actual.to_bits());
        }
    }
    let start = [0.; 3];
    let end = [6., 0., 0.];
    let outgoing = [10., 12., 0.];
    let main = Path::new(start, outgoing, [0.; 3], end, true);
    let branch = Path::new(start, outgoing, [0.; 3], end, false);
    let point = main.sample(0.25);
    anyhow::ensure!(point.into_iter().zip([1.3209865, 1.2202563, 0.]).all(|(actual, reference)| {
        (actual - reference).abs() <= 1e-6
    }), "Main spline did not use the chord-length table: {point:?}");
    anyhow::ensure!(branch.sample(0.25) == [2.34375, 1.6875, 0.],
        "Branch spline remapped the cubic phase");
    for path in [&main, &branch] {
        let full = path.duration(0., 1.);
        anyhow::ensure!(full > 6. && full == path.duration(0., 2.) * 2.,
            "Spline duration did not follow the curve length and live speed");
        anyhow::ensure!(path.duration(f32::EPSILON, 1.) == full
            && path.duration(2. * f32::EPSILON, 1.) == full / 200.
            && path.duration(8., 1.) == 8.,
            "Wrong authored spline time threshold or minimum");
    }
    let samples: Vec<(bool, bool, Vec<(f32, String)>)> = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/spline_easing_kernel.json"
    ))?;
    for (no_start, no_end, phases) in samples {
        for (phase, expected) in phases {
            let actual = ease_phase(phase, no_start, no_end).to_bits();
            anyhow::ensure!(actual == u32::from_str_radix(&expected, 16)?,
                "Spline easing differs at phase {phase}, flags {no_start}/{no_end}: {actual:08x} vs {expected}");
        }
    }
    let samples: Vec<([f32; 4], bool, (f32, i8, f32))> = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/spline_phase_kernel.json"
    ))?;
    for ([phase, dt, length, duration], reversed, (sampled, boundary, residual)) in samples {
        let actual = phase_step(phase, dt, length, duration, reversed);
        anyhow::ensure!(actual == PhaseStep { phase: sampled, boundary, residual },
            "Spline phase step differs at {phase}, dt {dt}, length {length}, duration {duration}, reverse {reversed}: {actual:?}");
    }
    println!("PASS: Wii spline position, easing and phase step fixtures match");
    Ok(())
}
