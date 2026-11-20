//! Script feedback: REL218c5c/217260 and f64c/c9f0/1e5e94.
//! The envelope follows the retail generator. Camera application is a
//! functional angular adapter; native camera-stack composition remains open.
use anyhow::{ensure, Result};
#[derive(Default)]
pub struct Runtime {
    pub rumble_remaining: [f32; 4],
    pub rumble_requests: u64,
    pub shake_requests: u64,
    shakes: Vec<Shake>,
}
struct Shake { args: [f32; 6], elapsed: f32 }
impl Runtime {
    pub fn rumble(&mut self, channel: u32, duration: f32, enabled: bool) {
        self.rumble_requests += 1;
        if enabled {
            if let Some(timer) = self.rumble_remaining.get_mut(channel as usize) {
                // Retail starts the motor when <=0; subsequent requests extend it.
                *timer = if *timer > 0. { *timer + duration } else { duration };
            }
        }
    }
    pub fn shake(&mut self, args: [f32; 6]) -> Result<()> {
        ensure!(args.iter().all(|x|x.is_finite()), "Nonfinite ShakeCamera argument");
        ensure!(args[0] > 0. && args[2] > 0. && args[2] < 1., "Unsupported degenerate shake duration/envelope");
        self.shake_requests += 1;
        self.shakes.push(Shake { args, elapsed: 0. });
        Ok(())
    }
    pub fn advance(&mut self, delta: f32) {
        for timer in &mut self.rumble_remaining { if *timer > 0. { *timer -= delta; } }
        for shake in &mut self.shakes { shake.elapsed += delta; }
        self.shakes.retain(|shake|shake.elapsed < shake.args[0]);
    }
    pub fn angles(&self) -> [f32; 2] {
        let mut result = [0.; 2];
        for shake in &self.shakes {
            let [duration, frequency, attack, power, x, y] = shake.args;
            let t = shake.elapsed / duration;
            let envelope = if t <= attack { t / attack } else { (1. - t)/(1. - attack) };
            let envelope = envelope.powf(power);
            let weight = envelope * envelope * (3. - 2. * envelope);
            let phase = std::f32::consts::TAU * t * (duration * frequency);
            result[0] += x * (phase * f32::from_bits(0x3f36db6e)).sin() * weight;
            result[1] += y * phase.sin() * weight;
        }
        result
    }
    pub fn apply(&self, mut frame: super::nif_camera::Frame) -> super::nif_camera::Frame {
        let angles = self.angles();
        fn rotate(v: [f32;3], axis: [f32;3], angle:f32)->[f32;3] {
            let (s,c)=angle.sin_cos();
            let dot=v.iter().zip(axis).map(|(a,b)|a*b).sum::<f32>();
            let cross=[axis[1]*v[2]-axis[2]*v[1],axis[2]*v[0]-axis[0]*v[2],axis[0]*v[1]-axis[1]*v[0]];
            std::array::from_fn(|i|v[i]*c+cross[i]*s+axis[i]*dot*(1.-c))
        }
        let up=frame.up;
        frame.left=rotate(frame.left,up,angles[0]);
        frame.direction=rotate(frame.direction,up,angles[0]);
        let left=frame.left;
        frame.direction=rotate(frame.direction,left,angles[1]);
        frame.up=rotate(frame.up,left,angles[1]);
        frame
    }
}
#[cfg(debug_assertions)]
pub fn validate() -> Result<()> {
    let mut state = Runtime::default();
    state.rumble(0, 0.4, true); state.rumble(0, 0.6, true);
    ensure!(state.rumble_remaining[0] == 1., "Rumble extension failed");
    state.rumble(1, 2., false);
    ensure!(state.rumble_remaining[1] == 0., "Disabled rumble started");
    state.shake([1.5, 3., 0.05, 4., 0.02, 0.045])?;
    ensure!(state.angles() == [0.;2], "Shake starts off envelope");
    state.advance(0.075);
    ensure!(state.angles().iter().all(|x|x.is_finite()) && state.angles()!=[0.;2], "Shake did not advance");
    state.advance(2.);
    ensure!(state.angles()==[0.;2] && state.rumble_remaining[0]<=0., "Feedback did not expire");
    println!("PASS: script feedback timers, disabled rumble, visible shake and expiration");
    Ok(())
}
