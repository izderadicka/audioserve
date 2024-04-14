use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

struct State {
    counter: AtomicU64,
    last_tick: AtomicU64,
}

pub struct Leaky {
    state: State,
    capacity: u64,
    rate: f32,
    start: Instant,
}

impl Leaky {
    /// Creates new Leaky (leaky bucket algorithm implementation
    /// https://en.wikipedia.org/wiki/Leaky_bucket
    ///
    /// Parameter `rate` (units/sec) is used to calculate all other parameters.
    /// Capacity is 110 % of rate, minimum is 1.
    pub fn new(rate: f32) -> Self {
        let capacity = rate.ceil() as u64;
        Leaky::new_with_params(rate, capacity)
    }

    /// Creates new Leaky with detailed parameters
    ///
    /// Parameters:
    /// rate - units/sec
    /// capacity -  capacity of the bucket
    pub fn new_with_params(rate: f32, capacity: u64) -> Self {
        assert!(capacity > 0);
        assert!(rate > 0.0);
        let start = Instant::now();
        let state = State {
            counter: AtomicU64::new(0),
            last_tick: AtomicU64::new(0),
        };

        Leaky {
            state,
            capacity,
            rate,
            start,
        }
    }

    /// Indicates that new unit has arrived and returns Result, if this one is still with rate/capacity

    /// Return results - Ok(x) if still within capacity (x is capacity now taken), Err otherwise.
    pub fn start_one(&self) -> Result<u64, u64> {
        let mut last_tick = self.state.last_tick.load(Ordering::Relaxed);
        let mut new_counter = loop {
            let tick = Instant::now();
            let msecs_from_start = tick.duration_since(self.start).as_millis() as u64;

            let msecs_from_last_tick = msecs_from_start - last_tick;
            let to_leak = ((msecs_from_last_tick as f32 / 1000.0) * self.rate).floor() as u64;
            let mut counter = self.state.counter.load(Ordering::Relaxed);
            if to_leak > 0 {
                counter = counter.saturating_sub(to_leak);
                match self.state.last_tick.compare_exchange_weak(
                    last_tick,
                    msecs_from_start,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break counter,
                    Err(t) => last_tick = t,
                }
            } else {
                break counter;
            }
        };

        let res = if new_counter < self.capacity {
            new_counter += 1;
            Ok(new_counter)
        } else {
            Err(new_counter)
        };
        self.state.counter.store(new_counter, Ordering::Relaxed);
        res
    }

    #[cfg(test)]
    /// Returns remaining capacity at use
    fn immediate_capacity(&self) -> u64 {
        self.capacity - self.state.counter.load(Ordering::Relaxed)
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
            assert!(res.is_ok(), "should be ok for {}", i);
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
