//! Movie hold state: RELbad38 input actions, bacf4..bad1c completion.
//! Host elapsed time stands in for the native timer; exact clock/input ordering
//! and original SkipMovieUpdate movie presentation remain unverified.
#[derive(Debug)]
pub struct Hold {
    pub seconds: f32,
    started: Option<f64>,
    pub elapsed: f32,
    pub completed: bool,
}
impl Hold {
    pub fn new(seconds: f32) -> Self {
        Self {seconds,started:None,elapsed:0.,completed:false}
    }
    pub fn update(&mut self, held: bool, now: f64) -> bool {
        if self.completed {return true;}
        if !held {self.started=None;self.elapsed=0.;return false;}
        let start=*self.started.get_or_insert(now);
        self.elapsed=(now-start) as f32;
        self.completed=self.elapsed>=self.seconds;
        self.completed
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn authored_three_second_hold_accepts_equality_and_not_an_earlier_sample() {
        let mut hold=Hold::new(3.);
        assert!(!hold.update(true,100.));
        assert!(!hold.update(true,102.99));
        assert!(hold.update(true,103.));
    }
    #[test] fn release_restarts_the_hold_instead_of_accumulating_separate_presses() {
        let mut hold=Hold::new(3.);
        assert!(!hold.update(true,0.));assert!(!hold.update(true,2.));
        assert!(!hold.update(false,2.1));assert_eq!(hold.elapsed,0.);
        assert!(!hold.update(true,3.));assert!(!hold.update(true,5.));
        assert!(hold.update(true,6.));
    }
    #[test] fn nonpositive_native_threshold_completes_on_first_held_sample() {
        for threshold in [0.,-1.] {assert!(Hold::new(threshold).update(true,0.));}
    }
}
