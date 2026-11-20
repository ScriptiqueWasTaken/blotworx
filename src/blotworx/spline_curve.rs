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
        // The main path sampler 3c10bc uses 200 chord lengths built at
        // 3c177c..3c1824. Branch paths go directly through the cubic phase.
        // With zero tangents, distance sampling just follows the straight line.
        if !self.main_path || (self.curve.incoming == [0.; 3] && self.curve.outgoing == [0.; 3]) {
            return;
        }
        let lengths = self.lengths.get_or_insert_with(|| Box::new([0.; ARC_STEPS]));
        let mut previous = self.curve.start;
        self.total = 0.;
        for (i, length) in lengths.iter_mut().enumerate() {
            let next = self.curve.sample((i + 1) as f32 / ARC_STEPS as f32);
            let [x, y, z] = std::array::from_fn(|axis| next[axis] - previous[axis]);
            *length = (x * x + y * y + z * z).sqrt();
            self.total += *length;
            previous = next;
        }
    }

    pub(super) fn sample(&mut self, start: Point, end: Point, phase: f32) -> Point {
        if self.curve.start != start || self.curve.end != end {
            self.curve.start = start;
            self.curve.end = end;
            self.rebuild_lengths();
        }
        if self.main_path && self.lengths.is_none() {
            return std::array::from_fn(|axis| start[axis] + (end[axis] - start[axis]) * phase);
        }
        let parameter = self.parameter(phase);
        self.curve.sample(parameter)
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
    let mut main = Path::new(start, outgoing, [0.; 3], end, true);
    let mut branch = Path::new(start, outgoing, [0.; 3], end, false);
    let point = main.sample(start, end, 0.25);
    anyhow::ensure!(point.into_iter().zip([1.3209865, 1.2202563, 0.]).all(|(actual, reference)| {
        (actual - reference).abs() <= 1e-6
    }), "Main spline did not use the chord-length table: {point:?}");
    anyhow::ensure!(branch.sample(start, end, 0.25) == [2.34375, 1.6875, 0.],
        "Branch spline remapped the cubic phase");
    println!("PASS: 27 spline position samples match the decoded Wii kernel at every coordinate bit");
    Ok(())
}
