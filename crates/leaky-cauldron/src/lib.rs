use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

struct State {
    counter: usize,
    last_tick: Instant,
}

pub struct Leaky {
    state: Arc<Mutex<State>>,
    capacity: usize,
    rate: f32,
}

impl Leaky {
    /// Creates new Leaky (leaky bucket algorithm implementation
    /// https://en.wikipedia.org/wiki/Leaky_bucket
    ///
    /// Parameter `rate` (units/sec) is used to calculate all other parameters.
    /// Capacity is 110 % of rate, minimum is 1.
    pub fn new(rate: f32) -> Self {
        let capacity = (rate * 1.1).ceil() as usize;
        Leaky::new_with_params(rate, capacity)
    }

    /// Creates new Leaky with detailed parameters
    ///
    /// Parameters:
    /// rate - units/sec
    /// capacity -  capacity of the bucket
    pub fn new_with_params(rate: f32, capacity: usize) -> Self {
        assert!(capacity > 0);
        assert!(rate > 0.0);
        let state = State {
            counter: 0,
            last_tick: Instant::now(),
        };
        let state = Arc::new(Mutex::new(state));

        Leaky {
            state,
            capacity,
            rate,
        }
    }

    /// Indicates that new unit has arrived and returns Result, if this one is still with rate/capacity

    /// Return results - Ok(x) if still within capacity (x is capacity now taken), Err otherwise.
    pub fn start_one(&self) -> Result<usize, usize> {
        let mut state = self.state.lock().expect("Poisoned lock");
        let tick = Instant::now();
        let secs_from_last_leak = tick.duration_since(state.last_tick).as_secs_f32();
        let to_leak = (secs_from_last_leak * self.rate).round() as usize;
        if to_leak > 0 {
            state.counter = state.counter.saturating_sub(to_leak);
            state.last_tick = tick;
        }

        if state.counter < self.capacity {
            state.counter += 1;
            return Ok(state.counter);
        } else {
            return Err(state.counter);
        }
    }

    #[cfg(test)]
    /// Returns remaining capacity at use
    fn immediate_capacity(&self) -> usize {
        let state = self.state.lock().expect("Poisoned lock");
        self.capacity - state.counter
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use tokio::time::sleep;

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_leaky_basic() {
        let leaky = Leaky::new_with_params(50.0, 50);
        for i in 1..=50 {
            let res = leaky.start_one();
            assert!(res.is_ok());
            let n = res.unwrap();
            assert_eq!(n, i)
        }
        //now leaky should be full
        for _i in 1..=10 {
            let res = leaky.start_one();
            if let Err(n) = res {
                assert_eq!(n, 50)
            } else {
                panic!("Leaky should be full")
            }
        }
        // wait a bit for leak:
        sleep(Duration::from_millis(20)).await;
        let res = leaky.start_one();
        if let Ok(n) = res {
            assert_eq!(n, 50, "should release one slot");
        } else {
            panic!("Slot was not released by leaky")
        }

        // wait bit more
        sleep(Duration::from_millis(300)).await;

        let res = leaky.start_one();
        if let Ok(n) = res {
            assert!(n <= 50 - 14, "taken slots should decrease by at least 14");
        } else {
            panic!("Slot was not released by leaky")
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_leaky_pausing() {
        let leaky = Leaky::new_with_params(100.0, 10);

        macro_rules! tst {
            () => {
                for _i in 1..=10 {
                    assert!(leaky.start_one().is_ok());
                }
                //should be full now
                assert!(leaky.start_one().is_err());
                sleep(Duration::from_millis(150)).await;
                assert!(leaky.start_one().is_ok());
                assert_eq!(leaky.immediate_capacity(), 9);
            };
        }

        tst!();

        sleep(Duration::from_millis(200)).await;
        // again

        tst!();
    }
}
